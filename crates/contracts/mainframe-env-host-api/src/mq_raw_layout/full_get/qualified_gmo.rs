//! GMO1 output staging through the same capture and generated writeback policy.
//! MQ9.4 row0015 q101830_; supplements q096710_146–184/q096715_1205–1268.
use super::*;
use crate::mq_mqi::{MqMqiOutcome, MqMqiOutput, MqMqiRequest, MqMqiRequestEnvelope, MqMqiResult};

impl MqRawCapture {
    /// Stage an actual qualified GET's defined QName into owned GMO1 scratch.
    /// `captured_bytes` is the caller's complete original group snapshot, including
    /// suffix; create this capture from that same snapshot. Every byte of scratch
    /// must still equal it before any output is staged. Only ResolvedQName may
    /// change, and None preserves its caller bytes rather than synthesizing blanks.
    /// Structure characters are trusted capture facts, never body Encoding/CCSID.
    /// Known result/status, original request binding and finite limits are checked
    /// by their existing authorities. This grants no admission or live authority.
    /// The whole group, including suffix, is bounded by the existing MQI buffer
    /// byte limit. This finite profile is ordinary batch/queue-manager owned.
    ///
    /// The installed bridge must join this prepared scratch with MD, body, length
    /// and status in its own final checked all-argument batch. This method does not
    /// provide cross-argument atomicity. The sole writer's final prefix copy has
    /// no allocation, callback or fallible work; input/ignored/suffix bytes survive.
    pub fn stage_qualified_full_get_gmo(
        &self,
        context: MqRawWritebackContext,
        request: &MqMqiRequestEnvelope,
        result: &MqMqiResult,
        captured_bytes: &[u8],
        scratch: &mut [u8],
    ) -> Result<(), MqRawProblem> {
        if self.layout.kind != MqRawLayoutKind::Gmo1 {
            return Err(MqRawProblem::FieldKind);
        }
        if context.call != MqMqiCall::Get
            || context.platform != MqRawPlatform::Zos
            || !context.single_queue
            || context.dynamic_model_open
        {
            return Err(MqRawProblem::OutputPending);
        }
        request
            .limits
            .validate()
            .map_err(|_| MqRawProblem::OutputPending)?;
        if self.capacity > request.limits.buffer_bytes
            || captured_bytes.len() != self.capacity
            || scratch.len() != self.capacity
        {
            return Err(MqRawProblem::Capacity);
        }
        if captured_bytes[..self.layout.prefix_bytes] != *self.prefix() || scratch != captured_bytes
        {
            return Err(MqRawProblem::StaleCapture);
        }
        if !matches!(request.request, MqMqiRequest::QualifiedFullGet(_))
            || request.context.owner.environment != crate::MqHostEnvironment::ZosBatch
            || request.context.syncpoint_owner != crate::MqSyncpointOwner::QueueManager
        {
            return Err(MqRawProblem::OutputPending);
        }
        request
            .validate()
            .map_err(|_| MqRawProblem::OutputPending)?;
        result
            .validate(request.limits)
            .and_then(|()| result.validate_reviewed_output_for(&request.request))
            .map_err(|_| MqRawProblem::OutputPending)?;
        let value = match &result.outcome {
            MqMqiOutcome::ReviewedOutput {
                output: MqMqiOutput::QualifiedFullGot(value),
                ..
            }
            | MqMqiOutcome::Completed {
                output: MqMqiOutput::QualifiedFullGot(value),
                ..
            } => value,
            _ => return Err(MqRawProblem::OutputPending),
        };
        let characters = match value.characters {
            MqMdCharacterEncoding::AsciiCompatible => MqRawCharacterEncoding::AsciiCompatible,
            MqMdCharacterEncoding::OwnedCp037 => MqRawCharacterEncoding::OwnedCp037,
        };
        if self.encoding.characters != characters {
            return Err(MqRawProblem::UnsupportedEncoding);
        }
        let observations = value
            .resolved_queue
            .as_ref()
            .map(|name| observed("ResolvedQName", MqRawFieldValue::Characters(name)));
        self.writeback(context, observations.as_slice(), scratch)
    }
}

#[cfg(test)]
mod tests;
