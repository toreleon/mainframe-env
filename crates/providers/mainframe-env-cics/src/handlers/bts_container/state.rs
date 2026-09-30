//! Bounded, versioned command-data rows. Writers arrive in later slices.

use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CONTAINER_SCHEMA: &str = "mainframe-env.cics.container@1";
const CHANNEL_SCHEMA: &str = "mainframe-env.cics.channel@1";
const MAX_CONTAINER_BYTES: usize = 65_536;
const MAX_CONTAINER_ROW_BYTES: usize = 96 * 1024;
pub(super) const MAX_CONTAINERS: usize = 256;

pub(super) fn supported_stored_ccsid(ccsid: Option<u16>) -> bool {
    matches!(ccsid, None | Some(37))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum ContainerOwner {
    Channel {
        execution: String,
        principal: String,
        run_unit: String,
        channel: String,
    },
    Process {
        process_type: String,
        process_name: String,
        root_activity_id: String,
    },
    Activity {
        process_type: String,
        process_name: String,
        root_activity_id: String,
        activity_id: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(in crate::service::handlers) enum ContainerDatatype {
    Bit,
    Character,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::service::handlers) struct ContainerValue {
    pub datatype: ContainerDatatype,
    pub ccsid: Option<u16>,
    pub read_only: bool,
    #[serde(with = "encoded_bytes")]
    pub bytes: Vec<u8>,
}

mod encoded_bytes {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(D::Error::custom)
    }
}

impl ContainerValue {
    fn validate(&self) -> Result<(), HostProblem> {
        if self.bytes.len() > MAX_CONTAINER_BYTES
            || !supported_stored_ccsid(self.ccsid)
            || self.datatype == ContainerDatatype::Bit && self.ccsid.is_some()
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContainerRow {
    schema: String,
    owner: ContainerOwner,
    name: String,
    value: ContainerValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelRow {
    schema: String,
    owner: ContainerOwner,
    #[serde(default)]
    creator_program: Option<String>,
}

pub(super) fn valid_name(name: &str, max: usize) -> bool {
    if max == 16 {
        return !name.is_empty()
            && name.chars().count() <= max
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(
                        character,
                        '$' | '@'
                            | '#'
                            | '.'
                            | '/'
                            | '-'
                            | '_'
                            | '%'
                            | '&'
                            | '?'
                            | '!'
                            | ':'
                            | '|'
                            | '"'
                            | '='
                            | '¬'
                            | ','
                            | ';'
                            | '<'
                            | '>'
                    )
            });
    }
    !name.is_empty()
        && name.len() <= max
        && name.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'@' | b'$' | b'#' | b'_' | b'-')
        })
}

fn digest(owner: &ContainerOwner) -> Result<String, HostProblem> {
    let encoded = serde_json::to_vec(owner).map_err(|_| HostProblem::InfrastructureFailure)?;
    let hash = Sha256::digest(encoded);
    Ok(hash.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn container_namespace(owner: &ContainerOwner) -> Result<String, HostProblem> {
    Ok(format!("cics-container-v1/{}", digest(owner)?))
}

pub(super) fn channel_key(owner: &ContainerOwner) -> Result<String, HostProblem> {
    digest(owner)
}

pub(super) fn decode_container(
    row: &ProviderStateRecord,
    owner: &ContainerOwner,
) -> Result<ContainerValue, HostProblem> {
    if row.namespace != container_namespace(owner)?
        || row.version == 0
        || row.payload.len() > MAX_CONTAINER_ROW_BYTES
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let saved: ContainerRow =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if saved.schema != CONTAINER_SCHEMA
        || saved.owner != *owner
        || saved.name != row.key
        || !valid_name(&saved.name, 16)
        || saved.value.validate().is_err()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(saved.value)
}

pub(super) fn decode_channel(
    row: &ProviderStateRecord,
    owner: &ContainerOwner,
) -> Result<(), HostProblem> {
    if row.namespace != "cics-channel-v1"
        || row.key != channel_key(owner)?
        || row.version == 0
        || row.payload.len() > 1024
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let saved: ChannelRow =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if saved.schema != CHANNEL_SCHEMA || saved.owner != *owner {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

pub(super) fn container_record(
    owner: &ContainerOwner,
    name: &str,
    value: &ContainerValue,
) -> Result<ProviderStateRecord, HostProblem> {
    if !valid_name(name, 16) {
        return Err(HostProblem::Malformed);
    }
    value.validate()?;
    let saved = ContainerRow {
        schema: CONTAINER_SCHEMA.into(),
        owner: owner.clone(),
        name: name.into(),
        value: value.clone(),
    };
    let record = ProviderStateRecord {
        namespace: container_namespace(owner)?,
        key: name.into(),
        version: 1,
        payload: serde_json::to_vec(&saved).map_err(|_| HostProblem::InfrastructureFailure)?,
    };
    if record.payload.len() > MAX_CONTAINER_ROW_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(record)
}

#[cfg(test)]
pub(super) fn channel_record(owner: &ContainerOwner) -> Result<ProviderStateRecord, HostProblem> {
    channel_record_with_program(owner, None)
}

pub(super) fn channel_record_with_program(
    owner: &ContainerOwner,
    creator_program: Option<&str>,
) -> Result<ProviderStateRecord, HostProblem> {
    if !matches!(owner, ContainerOwner::Channel { .. }) {
        return Err(HostProblem::Malformed);
    }
    let saved = ChannelRow {
        schema: CHANNEL_SCHEMA.into(),
        owner: owner.clone(),
        creator_program: creator_program.map(str::to_owned),
    };
    Ok(ProviderStateRecord {
        namespace: "cics-channel-v1".into(),
        key: channel_key(owner)?,
        version: 1,
        payload: serde_json::to_vec(&saved).map_err(|_| HostProblem::InfrastructureFailure)?,
    })
}

pub(super) fn channel_creator(
    row: &ProviderStateRecord,
    owner: &ContainerOwner,
) -> Result<Option<String>, HostProblem> {
    decode_channel(row, owner)?;
    let saved: ChannelRow =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(saved.creator_program)
}
