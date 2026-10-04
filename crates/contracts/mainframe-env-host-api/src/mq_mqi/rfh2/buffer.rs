use super::*;

/// Checked import observations; opaque tail bytes are retained exactly, never
/// interpreted as a property or silently converted by the descriptor's CCSID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRfh2Import {
    /// Complete ordinary property name, default MQPD and complete value.
    pub properties: Vec<(MqPropertyName, MqPropertyDescriptor, MqPropertyData)>,
    /// Exact bytes following the single leading RFH2 StrucLength.
    pub tail: Vec<u8>,
}
fn long(bytes: &[u8], offset: usize) -> Result<i32, MqPropertyProblem> {
    Ok(i32::from_be_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or(MqPropertyProblem::Value)?
            .try_into()
            .map_err(|_| MqPropertyProblem::Value)?,
    ))
}
/// Decode the finite flat custom-folder profile, after source-valid MD1 and
/// bounded capture. Invalid bytes fail; recognized unrepresented modes stay
/// Unsupported. This function grants no live handle or import permission.
pub fn mq_rfh2_decode(
    md: &MqMdValue,
    bytes: &[u8],
    limits: MqMqiLimits,
) -> Result<MqRfh2Import, MqPropertyProblem> {
    use generated::*;
    limits.validate().map_err(|_| MqPropertyProblem::Capacity)?;
    validate_md(md)?;
    if bytes.len() > limits.buffer_bytes || bytes.len() > i32::MAX as usize {
        return Err(MqPropertyProblem::Capacity);
    }
    if bytes.is_empty() {
        return Ok(MqRfh2Import {
            properties: Vec::new(),
            tail: Vec::new(),
        });
    }
    if md.fields().format != MQFMT_RF_HEADER_2 {
        return Err(MqPropertyProblem::Value);
    }
    let fixed = MQRFH_STRUC_LENGTH_FIXED_2 as usize;
    if bytes.len() < fixed || bytes[STRUCID_OFFSET..VERSION_OFFSET] != MQRFH_STRUC_ID {
        return Err(MqPropertyProblem::Value);
    }
    if long(bytes, VERSION_OFFSET)? != MQRFH_VERSION_2 {
        return Err(MqPropertyProblem::Unsupported);
    }
    let length =
        usize::try_from(long(bytes, STRUCLENGTH_OFFSET)?).map_err(|_| MqPropertyProblem::Value)?;
    if length < fixed || length > bytes.len() || length % 4 != 0 {
        return Err(MqPropertyProblem::Value);
    }
    if long(bytes, ENCODING_OFFSET)? != native()
        || ![MQCCSI_INHERIT, mq_property_profile_ccsid()]
            .contains(&long(bytes, CODEDCHARSETID_OFFSET)?)
        || bytes[FORMAT_OFFSET..FLAGS_OFFSET] != MQFMT_NONE
        || long(bytes, FLAGS_OFFSET)? != MQRFH_NONE
        || long(bytes, NAMEVALUECCSID_OFFSET)? != NAME_VALUE_CCSID
    {
        return Err(MqPropertyProblem::Unsupported);
    }
    let properties = if length == fixed {
        Vec::new()
    } else {
        let n = usize::try_from(long(bytes, fixed)?).map_err(|_| MqPropertyProblem::Value)?;
        let end = fixed
            .checked_add(4)
            .and_then(|p| p.checked_add(n))
            .ok_or(MqPropertyProblem::Capacity)?;
        if n == 0 || n % 4 != 0 || end > length {
            return Err(MqPropertyProblem::Value);
        }
        if end != length {
            return Err(MqPropertyProblem::Unsupported);
        }
        vec![xml::parse(&bytes[fixed + 4..end], limits.message)?]
    };
    Ok(MqRfh2Import {
        properties,
        tail: bytes[length..].to_vec(),
    })
}
/// Independently count the chosen formatter's exact required bytes before
/// allocating output. Capacity failure therefore need not truncate a generated
/// buffer to invent DataLength. No queue/body operation is implied.
pub fn mq_rfh2_required_length(
    name: &MqPropertyName,
    pd: &MqPropertyDescriptor,
    value: &MqPropertyData,
    limits: MqMqiLimits,
) -> Result<usize, MqPropertyProblem> {
    limits.validate().map_err(|_| MqPropertyProblem::Capacity)?;
    let xml = xml::required(name, pd, value, limits.message)?;
    let padded = xml.checked_add(3).ok_or(MqPropertyProblem::Capacity)? & !3;
    let length = (generated::MQRFH_STRUC_LENGTH_FIXED_2 as usize)
        .checked_add(4)
        .and_then(|n| n.checked_add(padded))
        .ok_or(MqPropertyProblem::Capacity)?;
    if length > limits.buffer_bytes || length > i32::MAX as usize {
        return Err(MqPropertyProblem::Capacity);
    }
    Ok(length)
}
/// Produce the explicit owned properties-only RFH2 choice. Tail triple is
/// native785/inherited CCSID/NONE; outer MD uses attested UTF8 at publication.
/// This is not an IBM byte-exact XML formatting assertion.
pub fn mq_rfh2_encode(
    name: &MqPropertyName,
    pd: &MqPropertyDescriptor,
    value: &MqPropertyData,
    limits: MqMqiLimits,
) -> Result<Vec<u8>, MqPropertyProblem> {
    use generated::*;
    let length = mq_rfh2_required_length(name, pd, value, limits)?;
    let fixed = MQRFH_STRUC_LENGTH_FIXED_2 as usize;
    let mut bytes = vec![0; length];
    bytes[STRUCID_OFFSET..VERSION_OFFSET].copy_from_slice(&MQRFH_STRUC_ID);
    for (offset, value) in [
        (VERSION_OFFSET, MQRFH_VERSION_2),
        (STRUCLENGTH_OFFSET, length as i32),
        (ENCODING_OFFSET, native()),
        (CODEDCHARSETID_OFFSET, MQCCSI_INHERIT),
        (FLAGS_OFFSET, MQRFH_NONE),
        (NAMEVALUECCSID_OFFSET, NAME_VALUE_CCSID),
        (fixed, (length - fixed - 4) as i32),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    bytes[FORMAT_OFFSET..FLAGS_OFFSET].copy_from_slice(&MQFMT_NONE);
    let xml = xml::render(name, value)?;
    bytes[fixed + 4..].fill(b' ');
    bytes[fixed + 4..fixed + 4 + xml.len()].copy_from_slice(xml.as_bytes());
    Ok(bytes)
}
