//! Strict schema adapters into the existing legacy authority's owned types.
//! Semantic validation remains `validate_state`/the existing replay validator.

use super::*;
use serde::de::{Error, MapAccess, Visitor};

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

#[derive(Deserialize)]
#[serde(remote = "RowStoreManifest", deny_unknown_fields)]
struct ManifestSchema {
    schema_version: String,
    #[serde(deserialize_with = "required_option")]
    definitions: Option<Vec<MqQueueDefinition>>,
    next_handle: u32,
}
pub(super) fn manifest(bytes: &[u8]) -> Result<RowStoreManifest, ReadError> {
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let value = ManifestSchema::deserialize(&mut d).map_err(|_| ReadError::Corrupt)?;
    d.end().map_err(|_| ReadError::Corrupt)?;
    Ok(value)
}

#[derive(Deserialize)]
#[serde(remote = "Message", deny_unknown_fields)]
struct MessageSchema {
    data: Vec<u8>,
    message_id: Vec<u8>,
    correlation_id: Vec<u8>,
}
#[derive(Deserialize)]
struct StrictMessage(#[serde(with = "MessageSchema")] Message);
fn messages<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Message>, D::Error> {
    Ok(Vec::<StrictMessage>::deserialize(d)?
        .into_iter()
        .map(|v| v.0)
        .collect())
}
#[derive(Deserialize)]
#[serde(remote = "Queue", deny_unknown_fields)]
struct QueueSchema {
    #[serde(deserialize_with = "required_option")]
    trigger_program: Option<String>,
    #[serde(deserialize_with = "messages")]
    messages: Vec<Message>,
}
#[derive(Deserialize)]
pub(super) struct StrictQueue(#[serde(with = "QueueSchema")] pub(super) Queue);

fn operations<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<(String, Message)>, D::Error> {
    Ok(Vec::<(String, StrictMessage)>::deserialize(d)?
        .into_iter()
        .map(|(q, v)| (q, v.0))
        .collect())
}
#[derive(Deserialize)]
#[serde(remote = "PendingUnit", deny_unknown_fields)]
struct PendingSchema {
    #[serde(deserialize_with = "operations")]
    puts: Vec<(String, Message)>,
    #[serde(deserialize_with = "operations")]
    gets: Vec<(String, Message)>,
}
#[derive(Deserialize)]
pub(super) struct StrictPending(#[serde(with = "PendingSchema")] pub(super) PendingUnit);

pub(super) struct StrictHandles(pub(super) BTreeMap<u32, String>);
impl<'de> Deserialize<'de> for StrictHandles {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Handles;
        impl<'de> Visitor<'de> for Handles {
            type Value = StrictHandles;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a map of canonical decimal legacy handles to queue names")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut handles = BTreeMap::new();
                while let Some((key, value)) = a.next_entry::<String, String>()? {
                    let id = key.parse::<u32>().map_err(A::Error::custom)?;
                    if id == 0 || key != id.to_string() || handles.insert(id, value).is_some() {
                        return Err(A::Error::custom("invalid or duplicate legacy handle"));
                    }
                }
                Ok(StrictHandles(handles))
            }
        }
        d.deserialize_map(Handles)
    }
}
