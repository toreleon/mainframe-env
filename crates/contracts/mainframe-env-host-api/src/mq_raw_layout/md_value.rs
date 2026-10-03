//! Projection through the sole generated layout; no copied offsets or defaults.
use super::*;
use crate::mq_md_value::{
    MqMdCharacterEncoding, MqMdFields, MqMdV2Fields, MqMdValue, MqMdValueProblem,
};

pub(crate) fn validate_md_identity(value: &MqMdValue) -> Result<(), MqMdValueProblem> {
    let layout = mq_raw_layout(match value {
        MqMdValue::V1 { .. } => MqRawLayoutKind::Md1,
        MqMdValue::V2 { .. } => MqRawLayoutKind::Md2,
    });
    let expected = match value.characters() {
        MqMdCharacterEncoding::AsciiCompatible => layout.ascii_identifier,
        MqMdCharacterEncoding::OwnedCp037 => layout.cp037_identifier,
    };
    if value.fields().struc_id == expected {
        Ok(())
    } else {
        Err(MqMdValueProblem::StructureIdentifier)
    }
}

impl MqRawCapture {
    /// Complete MQMD1/2 VALUE only. The old partial projection stays pending.
    /// Numeric byte order is supplied at capture, not selected by MQMD.Encoding.
    /// Capacity and any caller-owned suffix are never part of this value codec.
    pub fn to_full_md_value(&self) -> Result<MqMdValue, MqRawProblem> {
        if !matches!(
            self.layout.kind,
            MqRawLayoutKind::Md1 | MqRawLayoutKind::Md2
        ) {
            return Err(MqRawProblem::FieldKind);
        }
        let characters = match self.encoding.characters {
            MqRawCharacterEncoding::AsciiCompatible => MqMdCharacterEncoding::AsciiCompatible,
            MqRawCharacterEncoding::OwnedCp037 => MqMdCharacterEncoding::OwnedCp037,
            MqRawCharacterEncoding::Unsupported => return Err(MqRawProblem::UnsupportedEncoding),
        };
        let fields = MqMdFields {
            struc_id: chars(self, "StrucId")?,
            report: long(self, "Report")?,
            msg_type: long(self, "MsgType")?,
            expiry: long(self, "Expiry")?,
            feedback: long(self, "Feedback")?,
            encoding: long(self, "Encoding")?,
            coded_char_set_id: long(self, "CodedCharSetId")?,
            format: chars(self, "Format")?,
            priority: long(self, "Priority")?,
            persistence: long(self, "Persistence")?,
            msg_id: bytes(self, "MsgId")?,
            correl_id: bytes(self, "CorrelId")?,
            backout_count: long(self, "BackoutCount")?,
            reply_to_q: chars(self, "ReplyToQ")?,
            reply_to_q_mgr: chars(self, "ReplyToQMgr")?,
            user_identifier: chars(self, "UserIdentifier")?,
            accounting_token: bytes(self, "AccountingToken")?,
            appl_identity_data: chars(self, "ApplIdentityData")?,
            put_appl_type: long(self, "PutApplType")?,
            put_appl_name: chars(self, "PutApplName")?,
            put_date: chars(self, "PutDate")?,
            put_time: chars(self, "PutTime")?,
            appl_origin_data: chars(self, "ApplOriginData")?,
        };
        Ok(match long(self, "Version")? {
            1 if self.layout.kind == MqRawLayoutKind::Md1 => MqMdValue::V1 { characters, fields },
            2 if self.layout.kind == MqRawLayoutKind::Md2 => MqMdValue::V2 {
                characters,
                fields,
                extension: MqMdV2Fields {
                    group_id: bytes(self, "GroupId")?,
                    msg_seq_number: long(self, "MsgSeqNumber")?,
                    offset: long(self, "Offset")?,
                    msg_flags: long(self, "MsgFlags")?,
                    original_length: long(self, "OriginalLength")?,
                },
            },
            _ => return Err(MqRawProblem::Version),
        })
    }
}
fn long(capture: &MqRawCapture, name: &str) -> Result<i32, MqRawProblem> {
    match capture.field(name)? {
        MqRawFieldValue::Long(value) => Ok(value),
        _ => Err(MqRawProblem::FieldKind),
    }
}
fn chars<const N: usize>(capture: &MqRawCapture, name: &str) -> Result<[u8; N], MqRawProblem> {
    match capture.field(name)? {
        MqRawFieldValue::Characters(value) => {
            value.try_into().map_err(|_| MqRawProblem::FieldWidth)
        }
        _ => Err(MqRawProblem::FieldKind),
    }
}
fn bytes<const N: usize>(capture: &MqRawCapture, name: &str) -> Result<[u8; N], MqRawProblem> {
    match capture.field(name)? {
        MqRawFieldValue::Bytes(value) => value.try_into().map_err(|_| MqRawProblem::FieldWidth),
        _ => Err(MqRawProblem::FieldKind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mq_md_value::tests::{encoding, kind, raw};
    #[test]
    fn generated_field_kind_and_fixed_width_are_required() {
        let capture =
            MqRawCapture::capture(kind(false), &raw(false, true, false), encoding(true, false))
                .unwrap();
        assert_eq!(
            chars::<7>(&capture, "Format"),
            Err(MqRawProblem::FieldWidth)
        );
        assert_eq!(
            bytes::<23>(&capture, "MsgId"),
            Err(MqRawProblem::FieldWidth)
        );
        assert_eq!(long(&capture, "Format"), Err(MqRawProblem::FieldKind));
        assert_eq!(chars::<24>(&capture, "MsgId"), Err(MqRawProblem::FieldKind));
        assert_eq!(bytes::<8>(&capture, "Format"), Err(MqRawProblem::FieldKind));
        assert_eq!(long(&capture, "GroupId"), Err(MqRawProblem::Field));
    }
}
