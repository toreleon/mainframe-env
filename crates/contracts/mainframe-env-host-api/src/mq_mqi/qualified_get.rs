//! Additive complete GET observation; no live object or character authority.
use super::*;
use crate::MqTruncationDisposition as T;
use crate::mq_md_value::MqMdCharacterEncoding;

/// Lossless finite MQGET return including explicitly observed ResolvedQName.
/// Old FullGot remains unchanged. Absence requires preserving caller GMO bytes,
/// never writing blanks or synthesizing a name for an unestablished error field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiQualifiedGot {
    /// Independently configured structure profile; body CCSID does not select it.
    pub characters: MqMdCharacterEncoding,
    /// Existing complete/prefix/retained/absent observation.
    pub disposition: MqGetDisposition,
    /// Exact meaningful complete MD and copied application prefix.
    pub message: Option<MqFullMessage>,
    /// Actual defined nonnegative MQLONG application length.
    pub data_length: Option<i32>,
    /// Existing bounded cursor observation, never executable authority.
    pub cursor: Option<u64>,
    /// Exact MQCHAR48 local queue name for complete/accepted retrieval.
    /// Rejected truncation/no-message/unknown retain absence in this profile.
    pub resolved_queue: Option<[u8; 48]>,
}
impl MqMqiQualifiedGot {
    /// Bounded representation and definedness, not registry/SAF/status permission.
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMqiProblem> {
        super::full_message::got(
            self.disposition,
            self.message.as_ref(),
            self.data_length,
            self.cursor,
            limits,
        )?;
        let retrieved = matches!(
            self.disposition,
            MqGetDisposition::Message(T::Complete { .. } | T::AcceptedRemoved { .. })
        );
        if retrieved != self.resolved_queue.is_some()
            || matches!(
                self.disposition,
                MqGetDisposition::Message(T::AcceptedBrowsed { .. })
            )
            || self
                .message
                .as_ref()
                .is_some_and(|m| m.descriptor.characters() != self.characters)
            || (self.resolved_queue.is_some() && limits.destination_bytes < 48)
        {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        Ok(())
    }
}
pub(super) fn bind(request: &MqMqiRequest, output: &MqMqiOutput) -> Result<(), MqMqiProblem> {
    match (request, output) {
        (MqMqiRequest::QualifiedFullGet(get), MqMqiOutput::QualifiedFullGot(value)) => {
            if get.descriptor.characters() != value.characters {
                return Err(MqMqiProblem::OutputCallMismatch);
            }
            super::full_message::bind_get(get, value.disposition, value.message.as_ref())
        }
        (MqMqiRequest::QualifiedFullGet(_), _) | (_, MqMqiOutput::QualifiedFullGot(_)) => {
            Err(MqMqiProblem::OutputCallMismatch)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
