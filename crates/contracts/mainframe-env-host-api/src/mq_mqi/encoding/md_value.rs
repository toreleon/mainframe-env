//! Frozen full-MQMD VALUE codec. No old effect/replay domain or payload changes.
use super::*;
use crate::mq_md_value::*;

/// New value schema only; not an accepted HostRequest/checkpoint/replay schema.
pub const MQ_MD_VALUE_SCHEMA: &str = "mainframe-env.mq-md-value@1";
/// Distinct new canonical VALUE preimage domain, with its terminating zero byte.
pub const MQ_MD_VALUE_DOMAIN: &[u8] = b"mainframe-env.mq-md-value@1\0";
/// Finite complete-value preimage cap, independent of all old effect ceilings.
pub const MQ_MD_VALUE_MAX_BYTES: usize = 2048;

object!(MqMdFields {
    accounting_token,
    appl_identity_data,
    appl_origin_data,
    backout_count,
    coded_char_set_id,
    correl_id,
    encoding,
    expiry,
    feedback,
    format,
    msg_id,
    msg_type,
    persistence,
    priority,
    put_appl_name,
    put_appl_type,
    put_date,
    put_time,
    reply_to_q,
    reply_to_q_mgr,
    report,
    struc_id,
    user_identifier
});
object!(MqMdV2Fields {
    group_id,
    msg_flags,
    msg_seq_number,
    offset,
    original_length
});
variants!(MqMdCharacterEncoding {
    AsciiCompatible,
    OwnedCp037
});
variants!(MqMdValue {
    V1 { characters, fields },
    V2 { characters, extension, fields }
});
struct Boundary<'a>(&'a MqMdValue);
impl Canonical for Boundary<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.text(MQ_MD_VALUE_SCHEMA)?;
        self.0.encode(out)
    }
}
fn emit(
    value: &MqMdValue,
    limit: usize,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<usize, MqMdValueProblem> {
    value.validate_representation()?;
    crate::canonical::encode(
        &Boundary(value),
        MQ_MD_VALUE_DOMAIN,
        limit.min(MQ_MD_VALUE_MAX_BYTES),
        sink,
    )
    .map_err(|_| MqMdValueProblem::CanonicalLimit)
}
/// Count the complete frozen value preimage under both finite byte ceilings.
pub fn mq_md_value_size(value: &MqMdValue, byte_ceiling: usize) -> Result<usize, MqMdValueProblem> {
    emit(value, byte_ceiling, &mut |_| {})
}
/// Encode exact observations, counting before bounded allocation or emission.
pub fn mq_md_value_bytes(
    value: &MqMdValue,
    byte_ceiling: usize,
) -> Result<Vec<u8>, MqMdValueProblem> {
    let size = mq_md_value_size(value, byte_ceiling)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| MqMdValueProblem::Allocation)?;
    emit(value, byte_ceiling, &mut |part| {
        bytes.extend_from_slice(part)
    })?;
    Ok(bytes)
}
/// Hash the complete new VALUE preimage, never a replacement core effect digest.
pub fn mq_md_value_digest(
    value: &MqMdValue,
    byte_ceiling: usize,
) -> Result<[u8; 32], MqMdValueProblem> {
    let mut digest = Sha256::new();
    emit(value, byte_ceiling, &mut |part| digest.update(part))?;
    Ok(digest.finalize().into())
}

/// Strict fixed-shape decoder, without allocation or semantic normalization.
/// Names/order/count/tags/widths are exact; required fields cannot be omitted,
/// duplicated, reordered or replaced. Version1 has no extension/defaults.
pub fn mq_md_value_decode(
    bytes: &[u8],
    byte_ceiling: usize,
) -> Result<MqMdValue, MqMdValueProblem> {
    if bytes.len() > byte_ceiling.min(MQ_MD_VALUE_MAX_BYTES) {
        return Err(MqMdValueProblem::CanonicalLimit);
    }
    let mut input = Reader(bytes);
    input.exact(MQ_MD_VALUE_DOMAIN)?;
    input.name(MQ_MD_VALUE_SCHEMA)?;
    input.tag(0x41)?;
    input.name("MqMdValue")?;
    let version = match input.text()? {
        b"V1" => 1,
        b"V2" => 2,
        _ => return Err(MqMdValueProblem::UnsupportedVersion),
    };
    input.count(if version == 1 { 2 } else { 3 })?;
    input.name("characters")?;
    input.tag(0x41)?;
    input.name("MqMdCharacterEncoding")?;
    let characters = match input.text()? {
        b"AsciiCompatible" => MqMdCharacterEncoding::AsciiCompatible,
        b"OwnedCp037" => MqMdCharacterEncoding::OwnedCp037,
        _ => return Err(MqMdValueProblem::UnsupportedCharacterEncoding),
    };
    input.count(0)?;
    let extension = if version == 2 {
        input.name("extension")?;
        input.object("MqMdV2Fields", 5)?;
        Some(MqMdV2Fields {
            group_id: input.bytes("group_id")?,
            msg_flags: input.long("msg_flags")?,
            msg_seq_number: input.long("msg_seq_number")?,
            offset: input.long("offset")?,
            original_length: input.long("original_length")?,
        })
    } else {
        None
    };
    input.name("fields")?;
    input.object("MqMdFields", 23)?;
    let fields = MqMdFields {
        accounting_token: input.bytes("accounting_token")?,
        appl_identity_data: input.bytes("appl_identity_data")?,
        appl_origin_data: input.bytes("appl_origin_data")?,
        backout_count: input.long("backout_count")?,
        coded_char_set_id: input.long("coded_char_set_id")?,
        correl_id: input.bytes("correl_id")?,
        encoding: input.long("encoding")?,
        expiry: input.long("expiry")?,
        feedback: input.long("feedback")?,
        format: input.bytes("format")?,
        msg_id: input.bytes("msg_id")?,
        msg_type: input.long("msg_type")?,
        persistence: input.long("persistence")?,
        priority: input.long("priority")?,
        put_appl_name: input.bytes("put_appl_name")?,
        put_appl_type: input.long("put_appl_type")?,
        put_date: input.bytes("put_date")?,
        put_time: input.bytes("put_time")?,
        reply_to_q: input.bytes("reply_to_q")?,
        reply_to_q_mgr: input.bytes("reply_to_q_mgr")?,
        report: input.long("report")?,
        struc_id: input.bytes("struc_id")?,
        user_identifier: input.bytes("user_identifier")?,
    };
    if !input.0.is_empty() {
        return Err(MqMdValueProblem::Malformed);
    }
    let value = match extension {
        None => MqMdValue::V1 { characters, fields },
        Some(extension) => MqMdValue::V2 {
            characters,
            fields,
            extension,
        },
    };
    value.validate_representation()?;
    Ok(value)
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], MqMdValueProblem> {
        let (head, rest) = self
            .0
            .split_at_checked(size)
            .ok_or(MqMdValueProblem::Truncated)?;
        self.0 = rest;
        Ok(head)
    }
    fn exact(&mut self, expected: &[u8]) -> Result<(), MqMdValueProblem> {
        if self.take(expected.len())? == expected {
            Ok(())
        } else {
            Err(MqMdValueProblem::Malformed)
        }
    }
    fn tag(&mut self, tag: u8) -> Result<(), MqMdValueProblem> {
        self.exact(&[tag])
    }
    fn count(&mut self, count: u64) -> Result<(), MqMdValueProblem> {
        self.exact(&count.to_le_bytes())
    }
    fn text(&mut self) -> Result<&'a [u8], MqMdValueProblem> {
        self.tag(1)?;
        let length = u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| MqMdValueProblem::Truncated)?,
        );
        let length = usize::try_from(length).map_err(|_| MqMdValueProblem::CanonicalLimit)?;
        if length > MQ_MD_VALUE_MAX_BYTES {
            return Err(MqMdValueProblem::CanonicalLimit);
        }
        self.take(length)
    }
    fn name(&mut self, name: &str) -> Result<(), MqMdValueProblem> {
        if self.text()? == name.as_bytes() {
            Ok(())
        } else {
            Err(MqMdValueProblem::Malformed)
        }
    }
    fn object(&mut self, name: &str, count: u64) -> Result<(), MqMdValueProblem> {
        self.tag(0x40)?;
        self.name(name)?;
        self.count(count)
    }
    fn bytes<const N: usize>(&mut self, name: &str) -> Result<[u8; N], MqMdValueProblem> {
        self.name(name)?;
        self.tag(2)?;
        self.count(N as u64)?;
        self.take(N)?
            .try_into()
            .map_err(|_| MqMdValueProblem::Truncated)
    }
    fn long(&mut self, name: &str) -> Result<i32, MqMdValueProblem> {
        self.name(name)?;
        self.tag(0x1a)?;
        Ok(i32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| MqMdValueProblem::Truncated)?,
        ))
    }
}

#[cfg(test)]
mod tests;
