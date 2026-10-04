//! Finite source-reviewed RFH2 identity and observations. These values are not
//! live connection, CODESET, SAF, unit, or publication authority.

use super::property::{
    mq_property_initial_descriptor, mq_property_numeric_identities, mq_property_profile_ccsid,
};
use super::*;
use crate::mq_md_value::{MqMdCharacterEncoding, MqMdValue};
use crate::mq_status::{MqCompletion, MqReviewedStatus};
mod buffer;
mod generated;
mod observation;
mod xml;
pub use buffer::{MqRfh2Import, mq_rfh2_decode, mq_rfh2_encode, mq_rfh2_required_length};
pub use generated::MQ_RFH2_PROFILE_SHA256;
pub(super) use observation::bind;
pub use observation::{MqRfh2BufferObservation, MqRfh2Observation};

/// Immutable requested profile. A real connection must independently retain the
/// CODESET1208 source captured at original batch LE/DLL connection preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqRfh2Profile {
    /// ASCII-compatible structures, z/OS native numeric785, UTF-8 folders1208.
    /// It does not attest an installed LE runtime or select a host environment.
    ZosBatchUtf8NativeV1,
}

/// Checked explicit BMHO1/MHBO1 options; a value is not mutation permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqRfh2Options {
    pub(super) call: MqMqiCall,
    pub(super) version: i32,
    pub(super) options: i32,
}
impl MqRfh2Options {
    /// Admit explicit retain BUFMH or properties-in-RFH2 MHBUF with optional
    /// delete. Known unrepresented versions/modes remain Unsupported.
    pub fn checked(call: MqMqiCall, version: i32, options: i32) -> Result<Self, MqPropertyProblem> {
        use generated::*;
        let (v, known, accepted) = match call {
            MqMqiCall::BufferToHandle => (
                MQBMHO_VERSION_1,
                MQBMHO_DELETE_PROPERTIES,
                options == MQBMHO_NONE,
            ),
            MqMqiCall::HandleToBuffer => (
                MQMHBO_VERSION_1,
                MQMHBO_PROPERTIES_IN_MQRFH2 | MQMHBO_DELETE_PROPERTIES,
                options != MQMHBO_NONE
                    && (options == MQMHBO_PROPERTIES_IN_MQRFH2
                        || options == (MQMHBO_PROPERTIES_IN_MQRFH2 | MQMHBO_DELETE_PROPERTIES)),
            ),
            _ => return Err(MqPropertyProblem::Call),
        };
        if version != v {
            return Err(MqPropertyProblem::Unsupported);
        }
        if options < 0 || options & !known != 0 {
            return Err(MqPropertyProblem::Options);
        }
        if !accepted {
            return Err(MqPropertyProblem::Unsupported);
        }
        Ok(Self {
            call,
            version,
            options,
        })
    }
    /// Whether known successful MHBUF publication removes the selected property.
    /// A size failure, abort, or uncertain publication never authorizes deletion.
    pub fn deletes(self) -> bool {
        self.call == MqMqiCall::HandleToBuffer
            && self.options & generated::MQMHBO_DELETE_PROPERTIES != 0
    }
    /// Source structure identifier, without a native pointer or default filling.
    pub fn structure_id(self) -> [u8; 4] {
        if self.call == MqMqiCall::BufferToHandle {
            generated::MQBMHO_STRUC_ID
        } else {
            generated::MQMHBO_STRUC_ID
        }
    }
}

/// Exact original conversion request. Live tokens must come from the same
/// registry; aliases and retained replay observations cannot create authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiRfh2Request {
    /// Import one leading RFH2 into an empty ordinary property set and replace
    /// its associated MQMD1. Application descriptor/buffer remain unchanged.
    BufferToHandle {
        /// Actual issued ordinary parent connection.
        connection: MqHconn,
        /// Actual existing message handle on that parent and logical owner.
        handle: MqHmsg,
        /// Immutable requested profile, independently attested by the parent.
        profile: MqRfh2Profile,
        /// Explicit checked BMHO1 NONE; no guessed initializer.
        options: MqRfh2Options,
        /// Complete source-valid MQMD1 input, including opaque IDs.
        descriptor: MqMdValue,
        /// Entire declared BufferLength bytes; any tail stays opaque.
        buffer: Vec<u8>,
    },
    /// Format one exact custom ordinary property. No body or Root.MQMD XML is
    /// inserted; only the outer caller descriptor's body triple can change.
    HandleToBuffer {
        /// Actual issued ordinary parent connection.
        connection: MqHconn,
        /// Actual existing message handle.
        handle: MqHmsg,
        /// Immutable requested profile, independently attested by the parent.
        profile: MqRfh2Profile,
        /// Explicit checked MHBO1 properties-in-RFH2, with optional delete.
        options: MqRfh2Options,
        /// Complete caller MQMD1; unchanged fields are retained exactly.
        descriptor: MqMdValue,
        /// One exact nonreserved folder.leaf property; no wildcard/conversion.
        name: MqPropertyName,
        /// Actual writable byte capacity, independently bounded before dispatch.
        buffer_capacity: usize,
    },
}
impl MqMqiRfh2Request {
    /// Existing original MQI identity; this introduces no additional call.
    pub const fn call(&self) -> MqMqiCall {
        match self {
            Self::BufferToHandle { .. } => MqMqiCall::BufferToHandle,
            Self::HandleToBuffer { .. } => MqMqiCall::HandleToBuffer,
        }
    }
    /// Asserted live parent, never an attestation by itself.
    pub fn connection(&self) -> MqHconn {
        match self {
            Self::BufferToHandle { connection, .. } | Self::HandleToBuffer { connection, .. } => {
                *connection
            }
        }
    }
    /// Asserted live message identity; runtime lookup remains mandatory.
    pub fn handle(&self) -> MqHmsg {
        match self {
            Self::BufferToHandle { handle, .. } | Self::HandleToBuffer { handle, .. } => *handle,
        }
    }
    /// Named requested profile, not an application-selected source port.
    pub fn profile(&self) -> MqRfh2Profile {
        match self {
            Self::BufferToHandle { profile, .. } | Self::HandleToBuffer { profile, .. } => *profile,
        }
    }
    /// Complete input shape and product bounds. Malformed buffer content is an
    /// operation observation; recognized unsupported descriptor forms fail here.
    pub fn validate(&self, limits: MqMqiLimits) -> Result<(), MqPropertyProblem> {
        limits.validate().map_err(|_| MqPropertyProblem::Capacity)?;
        if !matches!(self.connection(), MqHconn::Issued(_)) {
            return Err(MqPropertyProblem::Unsupported);
        }
        let (options, md, size) = match self {
            Self::BufferToHandle {
                options,
                descriptor,
                buffer,
                ..
            } => (*options, descriptor, buffer.len()),
            Self::HandleToBuffer {
                options,
                descriptor,
                name,
                buffer_capacity,
                ..
            } => {
                custom_name(name, limits.message)?;
                (*options, descriptor, *buffer_capacity)
            }
        };
        if options.call != self.call() {
            return Err(MqPropertyProblem::Call);
        }
        MqRfh2Options::checked(options.call, options.version, options.options)?;
        validate_md(md)?;
        if size > limits.buffer_bytes || size > i32::MAX as usize {
            return Err(MqPropertyProblem::Capacity);
        }
        Ok(())
    }
}

pub(super) fn native() -> i32 {
    mq_property_numeric_identities()
        .iter()
        .find(|v| v.symbol == "MQENC_NATIVE_ZOS")
        .expect("reviewed property numeric authority")
        .value
}
pub(super) fn validate_md(md: &MqMdValue) -> Result<(), MqPropertyProblem> {
    let MqMdValue::V1 {
        characters: MqMdCharacterEncoding::AsciiCompatible,
        fields,
    } = md
    else {
        return Err(MqPropertyProblem::Unsupported);
    };
    let mut defaults = mq_property_initial_descriptor();
    let MqMdValue::V1 {
        fields: expected, ..
    } = &mut defaults
    else {
        unreachable!()
    };
    expected.encoding = fields.encoding;
    expected.coded_char_set_id = fields.coded_char_set_id;
    expected.format = fields.format;
    expected.msg_id = fields.msg_id;
    expected.correl_id = fields.correl_id;
    if &defaults != md
        || fields.encoding != native()
        || fields.coded_char_set_id != mq_property_profile_ccsid()
        || ![generated::MQFMT_NONE, generated::MQFMT_RF_HEADER_2].contains(&fields.format)
    {
        return Err(MqPropertyProblem::Unsupported);
    }
    Ok(())
}
pub(super) fn outer_md(md: &MqMdValue) -> MqMdValue {
    let mut md = md.clone();
    let MqMdValue::V1 { fields, .. } = &mut md else {
        unreachable!("validated MD1")
    };
    fields.encoding = native();
    fields.coded_char_set_id = mq_property_profile_ccsid();
    fields.format = generated::MQFMT_RF_HEADER_2;
    md
}
/// Checked outer MHBUF descriptor choice. Only Encoding/CCSID/Format change;
/// all other fields and opaque IDs remain the exact caller observations.
pub fn mq_rfh2_outer_descriptor(md: &MqMdValue) -> Result<MqMdValue, MqPropertyProblem> {
    validate_md(md)?;
    Ok(outer_md(md))
}
pub(super) fn custom_name(
    name: &MqPropertyName,
    limits: MqMessageLimits,
) -> Result<(), MqPropertyProblem> {
    MqPropertyName::checked(name.as_str().to_owned(), limits)?;
    let parts: Vec<_> = name.as_str().split('.').collect();
    if parts.len() != 2 {
        return Err(MqPropertyProblem::Unsupported);
    }
    for part in parts {
        if generated::RESERVED_NAMES
            .iter()
            .any(|n| part.eq_ignore_ascii_case(n))
            || generated::RESERVED_PREFIXES
                .iter()
                .any(|p| part.to_ascii_lowercase().starts_with(p))
            || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(MqPropertyProblem::Unsupported);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
