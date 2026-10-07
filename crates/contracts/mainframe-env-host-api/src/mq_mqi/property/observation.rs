use super::*;
use crate::mq_status::{MqCompletion, MqReviewedStatus};

/// Defined INQMP observations, including partial output on a source size failure.
/// Required sizes describe the actual stored property, never a truncated buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqPropertyInquiryObservation {
    /// Actual complete MQPD1 returned classification.
    pub descriptor: MqPropertyDescriptor,
    /// Actual returned type; copied numeric prefixes need not contain its full width.
    pub kind: MqPropertyType,
    /// Actual returned numeric encoding, not structure-decoder selection.
    pub returned_encoding: i32,
    /// Actual string value CCSID; absent when this output is undefined for nonstrings.
    pub returned_ccsid: Option<i32>,
    /// Returned exact name prefix; capacity need not hold the complete name.
    pub returned_name: Vec<u8>,
    /// Actual complete returned-name VSLength, in bytes.
    pub name_length: i32,
    /// Actual explicit returned-name CCSID, never the unresolved APPL sentinel.
    pub name_ccsid: i32,
    /// Actual complete value DataLength, including uncopied bytes on short failure.
    pub data_length: i32,
    /// Actual copied value prefix, preserving zero/high/null/padding bytes.
    pub copied_value: Vec<u8>,
}

/// Closed additional property output shapes; no output token creates authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqPropertyObservation {
    /// Defined SETMP MQPD output, including corrected descriptor CopyOptions.
    Set(MqPropertyDescriptor),
    /// Complete or source-size-failed INQMP observations.
    Inquired(MqPropertyInquiryObservation),
    /// Known successful descriptor reset or ordinary property deletion.
    PropertyDeleted,
    /// Defined DLTMH invalidation; the adapter must not manufacture a numeric alias.
    HandleDeleted,
    /// Property unavailable; undefined inquiry fields stay absent, not fabricated.
    Absent,
}
impl MqPropertyObservation {
    /// Standalone bounded shape check, independent of request/capacity and lifetime.
    pub fn validate(&self, call: MqMqiCall, limits: MqMqiLimits) -> Result<(), MqPropertyProblem> {
        let pd = |pd: &MqPropertyDescriptor| {
            if *pd == MqPropertyDescriptor::source_default()
                || *pd == MqPropertyDescriptor::descriptor_output()
            {
                Ok(())
            } else {
                Err(MqPropertyProblem::Value)
            }
        };
        match (self, call) {
            (Self::Set(descriptor), MqMqiCall::SetProperty) => pd(descriptor),
            (Self::Inquired(v), MqMqiCall::InquireProperty) => {
                pd(&v.descriptor)?;
                let name = usize::try_from(v.name_length).map_err(|_| MqPropertyProblem::Value)?;
                let value = usize::try_from(v.data_length).map_err(|_| MqPropertyProblem::Value)?;
                if name == 0
                    || name > limits.message.property_name_bytes
                    || value > limits.message.property_value_bytes
                    || v.returned_name.len() > name
                    || v.copied_value.len() > value
                {
                    return Err(MqPropertyProblem::Capacity);
                }
                if matches!(
                    v.kind,
                    MqPropertyType::Boolean | MqPropertyType::Float32 | MqPropertyType::Float64
                ) || generated::value_width(v.kind).is_some_and(|width| width != value)
                    || v.returned_encoding != generated::MQENC_NATIVE_ZOS
                    || v.name_ccsid != generated::PROPERTY_UTF8_CCSID
                    || v.returned_ccsid
                        != if v.kind == MqPropertyType::String {
                            Some(generated::PROPERTY_UTF8_CCSID)
                        } else {
                            None
                        }
                {
                    return Err(MqPropertyProblem::Value);
                }
                Ok(())
            }
            (Self::PropertyDeleted, MqMqiCall::DeleteProperty)
            | (Self::HandleDeleted, MqMqiCall::DeleteMessageHandle)
            | (Self::Absent, MqMqiCall::DeleteProperty | MqMqiCall::InquireProperty) => Ok(()),
            _ => Err(MqPropertyProblem::Call),
        }
    }
    /// Exact source-reviewed status/output pairing. It admits no mutation permission.
    pub fn validate_status(&self, status: MqReviewedStatus) -> bool {
        match (self, status.completion(), status.reason_symbol()) {
            (
                Self::Set(_) | Self::PropertyDeleted | Self::HandleDeleted,
                MqCompletion::Ok,
                "MQRC_NONE",
            ) => true,
            (Self::Inquired(v), MqCompletion::Ok, "MQRC_NONE") => {
                v.copied_value.len() == v.data_length as usize
                    && v.returned_name.len() == v.name_length as usize
            }
            (Self::Inquired(v), MqCompletion::Failed, "MQRC_PROPERTY_VALUE_TOO_BIG") => {
                v.copied_value.len() < v.data_length as usize
                    && v.returned_name.len() == v.name_length as usize
            }
            (Self::Inquired(v), MqCompletion::Failed, "MQRC_PROPERTY_NAME_TOO_BIG") => {
                v.returned_name.len() < v.name_length as usize
                    && v.copied_value.len() == v.data_length as usize
            }
            (Self::Absent, MqCompletion::Failed, "MQRC_PROPERTY_NOT_AVAILABLE") => {
                status.call() == MqMqiCall::InquireProperty
            }
            (Self::Absent, MqCompletion::Warning, "MQRC_PROPERTY_NOT_AVAILABLE") => {
                status.call() == MqMqiCall::DeleteProperty
            }
            _ => false,
        }
    }
}

/// Request/output relationship required before publication; older property DTOs
/// cannot consume these observations. This validates neither stored values nor SAF.
pub fn mq_property_bind(
    request: &MqMqiRequest,
    output: &MqMqiOutput,
) -> Result<(), MqPropertyProblem> {
    let MqMqiRequest::Property(request) = request else {
        return if matches!(output, MqMqiOutput::PropertyObservation(_)) {
            Err(MqPropertyProblem::Call)
        } else {
            Ok(())
        };
    };
    match (request, output) {
        (MqPropertyRequest::Create { .. }, MqMqiOutput::MessageHandle(_)) => Ok(()),
        (
            MqPropertyRequest::Set { name, .. },
            MqMqiOutput::PropertyObservation(MqPropertyObservation::Set(pd)),
        ) => {
            let expected = if name.descriptor_field().is_some() {
                MqPropertyDescriptor::descriptor_output()
            } else {
                MqPropertyDescriptor::source_default()
            };
            if *pd == expected {
                Ok(())
            } else {
                Err(MqPropertyProblem::Value)
            }
        }
        (
            MqPropertyRequest::Inquire {
                name,
                name_capacity,
                value_capacity,
                ..
            },
            MqMqiOutput::PropertyObservation(MqPropertyObservation::Inquired(v)),
        ) => {
            let expected = name.as_str().as_bytes();
            if v.name_length as usize != expected.len()
                || v.returned_name != expected[..expected.len().min(*name_capacity)]
                || v.copied_value.len() != (*value_capacity).min(v.data_length as usize)
            {
                return Err(MqPropertyProblem::Value);
            }
            let pd = if name.descriptor_field().is_some() {
                MqPropertyDescriptor::descriptor_output()
            } else {
                MqPropertyDescriptor::source_default()
            };
            if v.descriptor != pd {
                return Err(MqPropertyProblem::Value);
            }
            if let Some(field) = name.descriptor_field() {
                let definition = mq_property_md_fields()
                    .iter()
                    .find(|d| d.name == field)
                    .ok_or(MqPropertyProblem::Unsupported)?;
                if v.kind != definition.kind || v.data_length as usize != definition.width {
                    return Err(MqPropertyProblem::Value);
                }
            }
            Ok(())
        }
        (
            MqPropertyRequest::Delete { .. },
            MqMqiOutput::PropertyObservation(
                MqPropertyObservation::PropertyDeleted | MqPropertyObservation::Absent,
            ),
        )
        | (
            MqPropertyRequest::DeleteHandle { .. },
            MqMqiOutput::PropertyObservation(MqPropertyObservation::HandleDeleted),
        ) => Ok(()),
        (
            MqPropertyRequest::Inquire { name, .. },
            MqMqiOutput::PropertyObservation(MqPropertyObservation::Absent),
        ) if name.descriptor_field().is_none() => Ok(()),
        _ => Err(MqPropertyProblem::Call),
    }
}
