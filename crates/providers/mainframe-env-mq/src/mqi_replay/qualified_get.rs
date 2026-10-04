//! Sole codec @6 qualified GET projection, without live object/metadata authority.
use super::shape::{Get, required_option};
use super::*;
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    characters: String,
    disposition: Get,
    #[serde(deserialize_with = "required_option")]
    message: Option<super::full_message::Message>,
    #[serde(deserialize_with = "required_option")]
    data_length: Option<i32>,
    #[serde(deserialize_with = "required_option")]
    cursor: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    resolved_queue: Option<Vec<u8>>,
}
impl Observation {
    pub(super) fn capture(v: &MqMqiQualifiedGot) -> Result<Self, ReplayError> {
        Ok(Self {
            characters: match v.characters {
                MqMdCharacterEncoding::AsciiCompatible => "AsciiCompatible",
                MqMdCharacterEncoding::OwnedCp037 => "OwnedCp037",
            }
            .into(),
            disposition: v.disposition.into(),
            message: v
                .message
                .as_ref()
                .map(super::full_message::Message::capture)
                .transpose()?,
            data_length: v.data_length,
            cursor: v.cursor,
            resolved_queue: v.resolved_queue.map(|n| n.to_vec()),
        })
    }
    pub(super) fn restore(self) -> Result<MqMqiQualifiedGot, ReplayError> {
        Ok(MqMqiQualifiedGot {
            characters: match self.characters.as_str() {
                "AsciiCompatible" => MqMdCharacterEncoding::AsciiCompatible,
                "OwnedCp037" => MqMdCharacterEncoding::OwnedCp037,
                _ => return Err(ReplayError::Malformed),
            },
            disposition: self.disposition.into(),
            message: self
                .message
                .map(super::full_message::Message::restore)
                .transpose()?,
            data_length: self.data_length,
            cursor: self.cursor,
            resolved_queue: self
                .resolved_queue
                .map(|n| n.try_into().map_err(|_| ReplayError::Malformed))
                .transpose()?,
        })
    }
}
