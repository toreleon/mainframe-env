//! Sole result codec's @5 projection; no namespace or execution authority.
use super::shape::Delivery;
use super::*;

#[derive(Deserialize, Serialize)]
enum Count {
    UndefinedZos,
}
#[derive(Deserialize, Serialize)]
enum Ignored {
    PreservedIgnoredInput,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Produced {
    md_value: Vec<u8>,
    outcome: Delivery,
    resolved_queue: Vec<u8>,
    resolved_manager: Vec<u8>,
    known_dest_count: Count,
    unknown_dest_count: Count,
    invalid_dest_count: Count,
    backout_count: Ignored,
}
impl Produced {
    pub(super) fn capture(value: &MqMqiProduced) -> Result<Self, ReplayError> {
        Ok(Self {
            md_value: super::full_message::capture_md(&value.descriptor)?,
            outcome: value.outcome.clone().into(),
            resolved_queue: value.resolved_queue.to_vec(),
            resolved_manager: value.resolved_manager.to_vec(),
            known_dest_count: Count::UndefinedZos,
            unknown_dest_count: Count::UndefinedZos,
            invalid_dest_count: Count::UndefinedZos,
            backout_count: Ignored::PreservedIgnoredInput,
        })
    }
    pub(super) fn restore(self) -> Result<MqMqiProduced, ReplayError> {
        Ok(MqMqiProduced {
            descriptor: super::full_message::restore_md(&self.md_value)?,
            outcome: self.outcome.into(),
            resolved_queue: self
                .resolved_queue
                .try_into()
                .map_err(|_| ReplayError::Malformed)?,
            resolved_manager: self
                .resolved_manager
                .try_into()
                .map_err(|_| ReplayError::Malformed)?,
            known_dest_count: MqMqiDestinationCount::UndefinedZos,
            unknown_dest_count: MqMqiDestinationCount::UndefinedZos,
            invalid_dest_count: MqMqiDestinationCount::UndefinedZos,
            backout_count: MqMqiIgnoredCounter::PreservedIgnoredInput,
        })
    }
}
