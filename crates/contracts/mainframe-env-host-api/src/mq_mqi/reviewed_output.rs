//! Reviewed observations, never queue mutation or returned-handle authority.
//!
//! MQ 9.4 call baseline row0015 q101830_ (descriptor/buffer/DataLength,
//! lines 21–48; status lines 65–125) and programming-supplements q096715_
//! (MQGMO fields, lines 674–686). Both truncation reasons are WARNING in
//! the unchanged call table. Conversion and property size-reporting forms
//! without a lossless typed output remain unsupported here.
//! Rows0008/0009 q101760_/q101770_ also define WARNING/ALREADY_CONNECTED
//! with a returned connection (MQCONN usage271, CONNX return76–78). Shape
//! admission never proves that this is the actual prior live connection.

use super::*;
use crate::MqTruncationDisposition as T;
use crate::mq_status::{MqCompletion, MqReviewedStatus};

pub(super) fn validate(
    call: MqMqiCall,
    status: MqReviewedStatus,
    output: &MqMqiOutput,
    limits: MqMqiLimits,
) -> Result<(), MqMqiProblem> {
    if status.call() != call {
        return Err(MqMqiProblem::StatusCallMismatch);
    }
    if status.completion() == MqCompletion::Failed
        && status.reason_symbol() == "MQRC_ENVIRONMENT_ERROR"
    {
        return super::validation::validate_output(
            call,
            Some(MqMqiStatus::FailedEnvironment),
            output,
            limits,
        );
    }
    // Borrow the same exhaustive bounded shape authority used by old outcomes.
    // Its pending-mode shape check does not claim a reviewed completion.
    super::validation::validate_output(call, None, output, limits)?;
    let shape = match (status.completion(), status.reason_symbol(), output) {
        (
            MqCompletion::Ok,
            "MQRC_NONE",
            MqMqiOutput::Got { disposition, .. } | MqMqiOutput::FullGot { disposition, .. },
        ) => {
            matches!(disposition, MqGetDisposition::Message(T::Complete { .. }))
        }
        (
            MqCompletion::Ok,
            "MQRC_NONE",
            MqMqiOutput::Put { outcome, .. } | MqMqiOutput::FullPut { outcome, .. },
        ) => *outcome == MqDeliveryOutcome::Accepted,
        (MqCompletion::Ok, "MQRC_NONE", MqMqiOutput::Distribution(_)) => {
            // Per-destination return pairing is not represented by this output.
            false
        }
        (MqCompletion::Ok, "MQRC_NONE", _) => {
            return super::validation::validate_output(
                call,
                Some(MqMqiStatus::OkNone),
                output,
                limits,
            );
        }
        (MqCompletion::Warning, "MQRC_ALREADY_CONNECTED", MqMqiOutput::Connected(_)) => {
            matches!(call, MqMqiCall::Connect | MqMqiCall::ConnectExtended)
        }
        (
            MqCompletion::Warning,
            "MQRC_TRUNCATED_MSG_ACCEPTED",
            MqMqiOutput::Got { disposition, .. } | MqMqiOutput::FullGot { disposition, .. },
        ) => {
            matches!(
                disposition,
                MqGetDisposition::Message(T::AcceptedRemoved { .. } | T::AcceptedBrowsed { .. })
            )
        }
        (
            MqCompletion::Warning,
            "MQRC_TRUNCATED_MSG_FAILED",
            MqMqiOutput::Got {
                disposition,
                cursor,
                ..
            }
            | MqMqiOutput::FullGot {
                disposition,
                cursor,
                ..
            },
        ) => {
            // No browse-cursor advancement is reported for rejected truncation.
            cursor.is_none()
                && matches!(
                    disposition,
                    MqGetDisposition::Message(T::RejectedRetained { .. })
                )
        }
        (
            MqCompletion::Failed,
            "MQRC_NO_MSG_AVAILABLE",
            MqMqiOutput::Got { disposition, .. } | MqMqiOutput::FullGot { disposition, .. },
        ) => {
            matches!(
                disposition,
                MqGetDisposition::NoMessage | MqGetDisposition::WaitExpired
            )
        }
        _ => false,
    };
    if shape {
        Ok(())
    } else {
        Err(MqMqiProblem::StatusCallMismatch)
    }
}

impl MqMqiResult {
    /// Binds an admitted return and exact output to a validated original shape.
    /// This neither dispatches nor authorizes queue/UOW/handle changes.
    pub fn reviewed_output(
        status: MqReviewedStatus,
        output: MqMqiOutput,
        request: &MqMqiRequestEnvelope,
    ) -> Result<Self, MqMqiProblem> {
        request.validate()?;
        let result = Self {
            call: status.call(),
            outcome: MqMqiOutcome::ReviewedOutput { status, output },
        };
        result.validate(request.limits)?;
        result.validate_reviewed_output_for(&request.request)?;
        Ok(result)
    }

    /// Required in provider preflight even when the public enum was assembled
    /// directly. Standalone bounds and returned-handle registry checks remain
    /// separate obligations. Old outcome validation behavior is unchanged.
    pub fn validate_reviewed_output_for(&self, request: &MqMqiRequest) -> Result<(), MqMqiProblem> {
        if self.call != request.call() {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        if let MqMqiOutcome::ReviewedOutput { output, .. }
        | MqMqiOutcome::Completed { output, .. }
        | MqMqiOutcome::StatusPending { output } = &self.outcome
        {
            super::full_message::bind(request, output)?;
        }
        let MqMqiOutcome::ReviewedOutput { output, .. } = &self.outcome else {
            return Ok(());
        };
        match (request, output) {
            (
                MqMqiRequest::Get(request),
                MqMqiOutput::Got {
                    disposition,
                    message,
                    ..
                },
            ) => {
                if request.options != MqMqiOptions::ContractDefault {
                    return Err(MqMqiProblem::OutputCallMismatch);
                }
                disposition
                    .validate(&request.get)
                    .map_err(MqMqiProblem::Message)?;
                if let Some(message) = message {
                    if message.body.len() > request.get.buffer_capacity {
                        return Err(MqMqiProblem::Buffer);
                    }
                    if matches!(
                        disposition,
                        MqGetDisposition::Message(
                            T::RejectedRetained { .. }
                                | T::AcceptedRemoved { .. }
                                | T::AcceptedBrowsed { .. }
                        )
                    ) && message.body.len() != request.get.buffer_capacity
                    {
                        return Err(MqMqiProblem::Buffer);
                    }
                }
            }
            (
                MqMqiRequest::Inquire(request),
                MqMqiOutput::Attributes {
                    integers,
                    characters,
                },
            ) if integers.len() > request.integer_capacity
                || characters.len() > request.character_capacity =>
            {
                return Err(MqMqiProblem::Buffer);
            }
            (MqMqiRequest::InquireProperty(request), MqMqiOutput::Property(value))
                if value.name.len() > request.name_capacity
                    || value.value.len() > request.value_capacity =>
            {
                return Err(MqMqiProblem::Buffer);
            }
            (
                MqMqiRequest::BufferToHandle(request) | MqMqiRequest::HandleToBuffer(request),
                MqMqiOutput::Buffer { bytes, .. },
            ) if bytes.len() > request.capacity => {
                return Err(MqMqiProblem::Buffer);
            }
            _ => {}
        }
        Ok(())
    }
}
