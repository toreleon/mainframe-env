//! Storage@4 observations in the sole codec, without live CODESET/token authority.
use super::shape::required_option;
use super::*;

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Buffer {
    Unchanged {},
    WrittenPrefix { bytes: Vec<u8> },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    #[serde(deserialize_with = "required_option")]
    md_value: Option<Vec<u8>>,
    #[serde(deserialize_with = "required_option")]
    data_length: Option<i32>,
    buffer: Buffer,
}
impl Observation {
    pub(super) fn capture(v: &MqRfh2Observation) -> Result<Self, ReplayError> {
        Ok(Self {
            md_value: v
                .descriptor
                .as_ref()
                .map(super::full_message::capture_md)
                .transpose()?,
            data_length: v.data_length,
            buffer: match &v.buffer {
                MqRfh2BufferObservation::Unchanged => Buffer::Unchanged {},
                MqRfh2BufferObservation::WrittenPrefix(bytes) => Buffer::WrittenPrefix {
                    bytes: bytes.clone(),
                },
            },
        })
    }
    pub(super) fn restore(self) -> Result<MqRfh2Observation, ReplayError> {
        Ok(MqRfh2Observation {
            descriptor: self
                .md_value
                .map(|v| super::full_message::restore_md(&v))
                .transpose()?,
            data_length: self.data_length,
            buffer: match self.buffer {
                Buffer::Unchanged {} => MqRfh2BufferObservation::Unchanged,
                Buffer::WrittenPrefix { bytes } => MqRfh2BufferObservation::WrittenPrefix(bytes),
            },
        })
    }
}
