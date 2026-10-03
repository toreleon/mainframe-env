//! Finite producer observations. Neither shape nor PMO intent grants execution.
use super::*;
use crate::mq_md_value::{MqMdCharacterEncoding, MqMdValue};
use crate::mq_raw_layout::{MqRawInitialValue, MqRawLayoutKind, mq_raw_layout};
#[cfg(test)]
pub(crate) mod tests;

/// z/OS PMO destination counters have no defined numeric result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiDestinationCount {
    /// q098655_: distribution destination counts are undefined on z/OS.
    UndefinedZos,
}
/// Owned writeback policy for the MQPUT input-ignored BackoutCount observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiIgnoredCounter {
    /// Retain caller bytes in the result; a native writer must skip this field.
    /// This does not assert IBM returned the input or a source-defined output0.
    PreservedIgnoredInput,
}
/// Complete successful producer output, distinct from historical FullPut.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiProduced {
    /// Exact returned MD; input-ignored counter follows the explicit disposition.
    pub descriptor: MqMdValue,
    /// Accepted immediate put or pending current local unit.
    pub outcome: MqDeliveryOutcome,
    /// Source-defined resolved single local queue name, exact MQCHAR48 bytes.
    pub resolved_queue: [u8; 48],
    /// Source-defined resolved queue-manager name, exact MQCHAR48 bytes.
    pub resolved_manager: [u8; 48],
    /// Definedness, never fabricated local destination count1.
    pub known_dest_count: MqMqiDestinationCount,
    /// Definedness, never fabricated remote destination count0.
    pub unknown_dest_count: MqMqiDestinationCount,
    /// Definedness, never fabricated failed destination count0.
    pub invalid_dest_count: MqMqiDestinationCount,
    /// Explicit no-writeback observation for the ignored input counter.
    pub backout_count: MqMqiIgnoredCounter,
}

impl MqMqiFullPut {
    /// Checks the finite source-reviewed value profile, without registry, SAF,
    /// context/time, catalog, UOW or original core publication authority.
    /// Unsupported forms stay pending; the older ContractDefault is unchanged.
    pub fn validate_producer_profile(&self) -> Result<(), MqMqiProblem> {
        let f = self.message.descriptor.fields();
        let blank = match self.message.descriptor.characters() {
            MqMdCharacterEncoding::AsciiCompatible => b' ',
            MqMdCharacterEncoding::OwnedCp037 => 0x40,
        };
        let initial = |kind, name, value| {
            mq_raw_layout(kind)
                .fields
                .iter()
                .any(|field| field.name == name && field.initial == MqRawInitialValue::Long(value))
        };
        if self.options != MqMqiOptions::PutV1Synchronous
            || !matches!(
                self.context,
                MqMqiMessageContext::Default | MqMqiMessageContext::NoContext
            )
            || self.message_handle.is_some()
            || !self.message.properties.is_empty()
            || matches!(self.unit, MqMqiUnitOfWork::ExternalPending { .. })
            || !initial(MqRawLayoutKind::Md1, "MsgType", f.msg_type)
            || !initial(MqRawLayoutKind::Md1, "Report", f.report)
            || !initial(MqRawLayoutKind::Md1, "Feedback", f.feedback)
            || !initial(MqRawLayoutKind::Md1, "Expiry", f.expiry)
            || f.format != [blank; 8]
            || !matches!(f.persistence, 0 | 1)
            || f.priority != 0
            || !matches!(f.coded_char_set_id, 37 | 819)
            || f.msg_id == [0; 24]
            || f.reply_to_q != [blank; 48]
            || f.reply_to_q_mgr != [blank; 48]
        {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        if let MqMdValue::V2 { extension, .. } = &self.message.descriptor
            && (extension.group_id != [0; 24]
                || extension.msg_flags != 0
                || !initial(
                    MqRawLayoutKind::Md2,
                    "MsgSeqNumber",
                    extension.msg_seq_number,
                )
                || !initial(MqRawLayoutKind::Md2, "Offset", extension.offset)
                || !initial(
                    MqRawLayoutKind::Md2,
                    "OriginalLength",
                    extension.original_length,
                ))
        {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        Ok(())
    }
}
impl MqMqiProduced {
    pub(super) fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMqiProblem> {
        super::full_message::descriptor(&self.descriptor, limits)?;
        if !matches!(
            self.outcome,
            MqDeliveryOutcome::Accepted | MqDeliveryOutcome::Pending
        ) {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        let blank = match self.descriptor.characters() {
            MqMdCharacterEncoding::AsciiCompatible => b' ',
            MqMdCharacterEncoding::OwnedCp037 => 0x40,
        };
        if self.resolved_queue == [blank; 48] || self.resolved_manager == [blank; 48] {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        Ok(())
    }
    pub(super) fn bind(&self, put: &MqMqiFullPut) -> Result<(), MqMqiProblem> {
        put.validate_producer_profile()?;
        if (self.outcome == MqDeliveryOutcome::Pending)
            != matches!(put.unit, MqMqiUnitOfWork::Local { .. })
        {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        let mut expected = put.message.descriptor.clone();
        let fields = match &mut expected {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
        };
        let out = self.descriptor.fields();
        fields.user_identifier = out.user_identifier;
        fields.accounting_token = out.accounting_token;
        fields.appl_identity_data = out.appl_identity_data;
        fields.put_appl_type = out.put_appl_type;
        fields.put_appl_name = out.put_appl_name;
        fields.put_date = out.put_date;
        fields.put_time = out.put_time;
        fields.appl_origin_data = out.appl_origin_data;
        if expected != self.descriptor {
            return Err(MqMqiProblem::OutputCallMismatch);
        }
        if put.context == MqMqiMessageContext::Default {
            let blank = match expected.characters() {
                MqMdCharacterEncoding::AsciiCompatible => b' ',
                MqMdCharacterEncoding::OwnedCp037 => 0x40,
            };
            if out.put_appl_type != 2
                || out.appl_identity_data != [blank; 32]
                || out.appl_origin_data != [blank; 4]
            {
                return Err(MqMqiProblem::OutputCallMismatch);
            }
        }
        if put.context == MqMqiMessageContext::NoContext {
            let blank = match expected.characters() {
                MqMdCharacterEncoding::AsciiCompatible => b' ',
                MqMdCharacterEncoding::OwnedCp037 => 0x40,
            };
            if out.user_identifier != [blank; 12]
                || out.accounting_token != [0; 32]
                || out.appl_identity_data != [blank; 32]
                || out.put_appl_type != 0
                || out.put_appl_name != [blank; 28]
                || out.put_date != [blank; 8]
                || out.put_time != [blank; 8]
                || out.appl_origin_data != [blank; 4]
            {
                return Err(MqMqiProblem::OutputCallMismatch);
            }
        }
        Ok(())
    }
}
