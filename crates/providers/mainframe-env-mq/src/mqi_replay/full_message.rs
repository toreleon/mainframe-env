//! Complete-message storage composition through the existing MD/property codecs.
use super::*;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    md_value: Vec<u8>,
    body: Vec<u8>,
    properties: Vec<ReplayProperty>,
}
pub(super) fn capture_md(
    value: &mainframe_env_host_api::mq_md_value::MqMdValue,
) -> Result<Vec<u8>, ReplayError> {
    mq_md_value_bytes(value, MQ_MD_VALUE_MAX_BYTES)
        .map_err(|e| ReplayError::Mqi(MqMqiProblem::FullDescriptor(e)))
}
pub(super) fn restore_md(
    value: &[u8],
) -> Result<mainframe_env_host_api::mq_md_value::MqMdValue, ReplayError> {
    mq_md_value_decode(value, MQ_MD_VALUE_MAX_BYTES)
        .map_err(|e| ReplayError::Mqi(MqMqiProblem::FullDescriptor(e)))
}
impl Message {
    pub(super) fn capture(value: &MqFullMessage) -> Result<Self, ReplayError> {
        Ok(Self {
            md_value: capture_md(&value.descriptor)?,
            body: value.body.clone(),
            properties: value
                .properties
                .iter()
                .map(ReplayProperty::from_property)
                .collect(),
        })
    }
    pub(super) fn restore(self) -> Result<MqFullMessage, ReplayError> {
        Ok(MqFullMessage {
            descriptor: restore_md(&self.md_value)?,
            body: self.body,
            properties: self
                .properties
                .into_iter()
                .map(ReplayProperty::into_property)
                .collect::<Result<_, _>>()?,
        })
    }
}
