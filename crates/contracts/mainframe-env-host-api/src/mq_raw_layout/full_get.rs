//! Complete observed GET descriptor writeback through the sole generated policy.
use super::*;
use crate::mq_md_value::{MqMdCharacterEncoding, MqMdValue};

mod qualified_gmo;

impl MqRawCapture {
    /// Copy every observed MQMD1/2 output field atomically into its captured prefix.
    /// The caller supplies an actual returned descriptor, never a status-derived
    /// default. Version and structure character profile must match the input.
    /// Input-only StrucId/Version and caller-owned suffix bytes remain unchanged.
    /// This is a value writer, not GET admission, conversion or execution proof.
    pub fn writeback_full_get_md(
        &self,
        context: MqRawWritebackContext,
        descriptor: &MqMdValue,
        destination: &mut [u8],
    ) -> Result<(), MqRawProblem> {
        if context.call != MqMqiCall::Get {
            return Err(MqRawProblem::OutputPending);
        }
        let expected_kind = match descriptor {
            MqMdValue::V1 { .. } => MqRawLayoutKind::Md1,
            MqMdValue::V2 { .. } => MqRawLayoutKind::Md2,
        };
        if !matches!(
            self.layout.kind,
            MqRawLayoutKind::Md1 | MqRawLayoutKind::Md2
        ) {
            return Err(MqRawProblem::FieldKind);
        }
        if self.layout.kind != expected_kind {
            return Err(MqRawProblem::Version);
        }
        let expected_characters = match descriptor.characters() {
            MqMdCharacterEncoding::AsciiCompatible => MqRawCharacterEncoding::AsciiCompatible,
            MqMdCharacterEncoding::OwnedCp037 => MqRawCharacterEncoding::OwnedCp037,
        };
        if self.encoding.characters != expected_characters {
            return Err(MqRawProblem::UnsupportedEncoding);
        }
        validate_md_identity(descriptor).map_err(|_| MqRawProblem::StructureIdentifier)?;
        let fields = descriptor.fields();
        let common = [
            observed("Report", MqRawFieldValue::Long(fields.report)),
            observed("MsgType", MqRawFieldValue::Long(fields.msg_type)),
            observed("Expiry", MqRawFieldValue::Long(fields.expiry)),
            observed("Feedback", MqRawFieldValue::Long(fields.feedback)),
            observed("Encoding", MqRawFieldValue::Long(fields.encoding)),
            observed(
                "CodedCharSetId",
                MqRawFieldValue::Long(fields.coded_char_set_id),
            ),
            observed("Format", MqRawFieldValue::Characters(&fields.format)),
            observed("Priority", MqRawFieldValue::Long(fields.priority)),
            observed("Persistence", MqRawFieldValue::Long(fields.persistence)),
            observed("MsgId", MqRawFieldValue::Bytes(&fields.msg_id)),
            observed("CorrelId", MqRawFieldValue::Bytes(&fields.correl_id)),
            observed("BackoutCount", MqRawFieldValue::Long(fields.backout_count)),
            observed("ReplyToQ", MqRawFieldValue::Characters(&fields.reply_to_q)),
            observed(
                "ReplyToQMgr",
                MqRawFieldValue::Characters(&fields.reply_to_q_mgr),
            ),
            observed(
                "UserIdentifier",
                MqRawFieldValue::Characters(&fields.user_identifier),
            ),
            observed(
                "AccountingToken",
                MqRawFieldValue::Bytes(&fields.accounting_token),
            ),
            observed(
                "ApplIdentityData",
                MqRawFieldValue::Characters(&fields.appl_identity_data),
            ),
            observed("PutApplType", MqRawFieldValue::Long(fields.put_appl_type)),
            observed(
                "PutApplName",
                MqRawFieldValue::Characters(&fields.put_appl_name),
            ),
            observed("PutDate", MqRawFieldValue::Characters(&fields.put_date)),
            observed("PutTime", MqRawFieldValue::Characters(&fields.put_time)),
            observed(
                "ApplOriginData",
                MqRawFieldValue::Characters(&fields.appl_origin_data),
            ),
        ];
        match descriptor {
            MqMdValue::V1 { .. } => self.writeback(context, &common, destination),
            MqMdValue::V2 { extension, .. } => {
                let mut observations = Vec::new();
                observations
                    .try_reserve_exact(common.len() + 5)
                    .map_err(|_| MqRawProblem::Capacity)?;
                observations.extend_from_slice(&common);
                observations.extend_from_slice(&[
                    observed("GroupId", MqRawFieldValue::Bytes(&extension.group_id)),
                    observed(
                        "MsgSeqNumber",
                        MqRawFieldValue::Long(extension.msg_seq_number),
                    ),
                    observed("Offset", MqRawFieldValue::Long(extension.offset)),
                    observed("MsgFlags", MqRawFieldValue::Long(extension.msg_flags)),
                    observed(
                        "OriginalLength",
                        MqRawFieldValue::Long(extension.original_length),
                    ),
                ]);
                self.writeback(context, &observations, destination)
            }
        }
    }
}

fn observed<'a>(field: &'static str, value: MqRawFieldValue<'a>) -> MqRawObservedField<'a> {
    MqRawObservedField {
        field,
        observation: MqRawObservation::Observed(value),
    }
}

#[cfg(test)]
mod tests;
