//! Complete message boundary; representation is never per-call execution permission.
use super::*;
use crate::mq_md_value::MqMdValue;
use crate::{MqGetMode, MqMessageMatch, MqTruncation, MqTruncationDisposition, MqWait};

/// Complete descriptor, exact body and ordered shared typed properties.
/// No conversion to the old partial descriptor or message is provided.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqFullMessage {
    /// Every MQMD1/2 observation, including explicit structure character encoding.
    pub descriptor: MqMdValue,
    /// Exact application bytes; body Encoding/CCSID cannot decode the structure.
    pub body: Vec<u8>,
    /// Original property order and typed byte values, without property conversion.
    pub properties: Vec<MqMessageProperty>,
}

/// Complete put intent. Options/context/unit remain existing typed observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiFullPut {
    /// Complete message, never narrowed to the old delivery schema.
    pub message: MqFullMessage,
    /// Independently admitted live message handle required at execution.
    pub message_handle: Option<MqHmsg>,
    /// Context intent; supplied descriptor context is not a SAF assertion.
    pub context: MqMqiMessageContext,
    /// No new numeric option legality is inferred here.
    pub options: MqMqiOptions,
    /// Existing UOW intent, not durable ownership or a coordinator permit.
    pub unit: MqMqiUnitOfWork,
}

/// Complete MQGET descriptor input and explicit bounded existing controls.
/// Descriptor identifiers are not duplicated into the old partial selector DTO.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiFullGet {
    /// Independently admitted connection, never manufactured from numeric bits.
    pub connection: MqHconn,
    /// Independently admitted input/browse object.
    pub object: MqHobj,
    /// Exact input descriptor. MQGMO matching/conversion policy remains pending.
    pub descriptor: MqMdValue,
    /// Existing removal/browse intent, not a native numeric option bag.
    pub mode: MqGetMode,
    /// Existing finite wait intent; no new time conversion is inferred.
    pub wait: MqWait,
    /// Explicit acceptance or rejection of truncation.
    pub truncation: MqTruncation,
    /// Requested copied-byte capacity, including zero.
    pub buffer_capacity: usize,
    /// Optional existing message-handle intent, requiring live registry checks.
    pub message_handle: Option<MqHmsg>,
    /// Unrepresented option structures remain pending.
    pub options: MqMqiOptions,
    /// Existing unit intent requiring independent ownership at execution.
    pub unit: MqMqiUnitOfWork,
}

pub(super) fn descriptor(value: &MqMdValue, limits: MqMessageLimits) -> Result<(), MqMqiProblem> {
    value
        .validate_representation()
        .map_err(MqMqiProblem::FullDescriptor)?;
    if limits.identifier_bytes < 24 || limits.format_bytes < 8 {
        return Err(MqMqiProblem::Limits);
    }
    Ok(())
}
impl MqFullMessage {
    /// Exact representation/property/budget checks only; unknown MD policy is pending.
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMqiProblem> {
        limits.validate().map_err(MqMqiProblem::Message)?;
        descriptor(&self.descriptor, limits)?;
        if self.body.len() > limits.body_bytes || self.body.len() > i32::MAX as usize {
            return Err(MqMqiProblem::Buffer);
        }
        if self.properties.len() > limits.properties {
            return Err(MqMqiProblem::Message(
                crate::MqMessageProblem::PropertyCount,
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        let mut total = 0usize;
        for property in &self.properties {
            super::validation::property(property, limits)?;
            if !names.insert(property.name.as_str()) {
                return Err(MqMqiProblem::Message(
                    crate::MqMessageProblem::DuplicateProperty,
                ));
            }
            total = total
                .checked_add(property.name.len())
                .and_then(|n| n.checked_add(property.value.len()))
                .ok_or(MqMqiProblem::Limits)?;
            if total > limits.property_total_bytes {
                return Err(MqMqiProblem::Message(
                    crate::MqMessageProblem::PropertyTotalLength,
                ));
            }
        }
        Ok(())
    }
}
impl MqMqiFullGet {
    pub(crate) fn controls(&self) -> MqGetContract {
        MqGetContract {
            selection: MqMessageMatch::default(),
            mode: self.mode,
            wait: self.wait,
            truncation: self.truncation,
            buffer_capacity: self.buffer_capacity,
        }
    }
}
pub(super) fn unit(value: MqMqiUnitOfWork) -> Result<(), MqMqiProblem> {
    match value {
        MqMqiUnitOfWork::NoSyncpoint => Ok(()),
        MqMqiUnitOfWork::Local { unit } | MqMqiUnitOfWork::ExternalPending { unit }
            if unit != 0 && unit <= i64::MAX as u64 =>
        {
            Ok(())
        }
        _ => Err(MqMqiProblem::Unit),
    }
}

pub(super) fn got(
    disposition: MqGetDisposition,
    message: Option<&MqFullMessage>,
    data_length: Option<i32>,
    cursor: Option<u64>,
    limits: MqMessageLimits,
) -> Result<(), MqMqiProblem> {
    if cursor.is_some_and(|n| n == 0 || n > i64::MAX as u64) {
        return Err(MqMqiProblem::Buffer);
    }
    if let Some(message) = message {
        message.validate(limits)?;
    }
    let (required, copied) = match disposition {
        MqGetDisposition::Message(MqTruncationDisposition::Complete { length }) => (length, length),
        MqGetDisposition::Message(
            MqTruncationDisposition::RejectedRetained { required, copied }
            | MqTruncationDisposition::AcceptedRemoved { required, copied }
            | MqTruncationDisposition::AcceptedBrowsed { required, copied },
        ) if copied < required => (required, copied),
        MqGetDisposition::NoMessage
        | MqGetDisposition::WaitExpired
        | MqGetDisposition::UnknownOutcome
            if message.is_none() && data_length.is_none() && cursor.is_none() =>
        {
            return Ok(());
        }
        _ => return Err(MqMqiProblem::OutputCallMismatch),
    };
    if required > limits.body_bytes
        || i32::try_from(required).ok() != data_length
        || message.is_none_or(|m| m.body.len() != copied)
    {
        return Err(MqMqiProblem::Buffer);
    }
    Ok(())
}

pub(super) fn bind(request: &MqMqiRequest, output: &MqMqiOutput) -> Result<(), MqMqiProblem> {
    match (request, output) {
        (
            MqMqiRequest::FullPut { put, .. } | MqMqiRequest::FullPutOne { put, .. },
            MqMqiOutput::Produced(value),
        ) => value.bind(put)?,
        (
            MqMqiRequest::FullGet(get),
            MqMqiOutput::FullGot {
                disposition,
                message,
                ..
            },
        ) => {
            if get.options != MqMqiOptions::ContractDefault {
                return Err(MqMqiProblem::OutputCallMismatch);
            }
            disposition
                .validate(&get.controls())
                .map_err(MqMqiProblem::Message)?;
            if message.as_ref().is_some_and(|m| {
                m.descriptor.version() != get.descriptor.version()
                    || m.descriptor.characters() != get.descriptor.characters()
                    || m.body.len() > get.buffer_capacity
                    || (matches!(
                        disposition,
                        MqGetDisposition::Message(
                            MqTruncationDisposition::RejectedRetained { .. }
                                | MqTruncationDisposition::AcceptedRemoved { .. }
                                | MqTruncationDisposition::AcceptedBrowsed { .. }
                        )
                    ) && m.body.len() != get.buffer_capacity)
            }) {
                return Err(MqMqiProblem::Buffer);
            }
        }
        (
            MqMqiRequest::FullPut { put, .. } | MqMqiRequest::FullPutOne { put, .. },
            MqMqiOutput::FullPut { descriptor, .. },
        ) => {
            if put.options != MqMqiOptions::ContractDefault
                || descriptor.version() != put.message.descriptor.version()
                || descriptor.characters() != put.message.descriptor.characters()
            {
                return Err(MqMqiProblem::OutputCallMismatch);
            }
        }
        (
            MqMqiRequest::FullGet(_)
            | MqMqiRequest::FullPut { .. }
            | MqMqiRequest::FullPutOne { .. },
            _,
        )
        | (
            _,
            MqMqiOutput::FullPut { .. } | MqMqiOutput::FullGot { .. } | MqMqiOutput::Produced(_),
        ) => {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;
