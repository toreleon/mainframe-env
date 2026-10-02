//! Complete MQMD1/2 observations, distinct from the older partial descriptor.
//!
//! Fixed characters retain their exact bytes and explicit structure character
//! encoding. Body Encoding/CCSID never select that encoding. Signed MQLONGs are
//! observations, not admitted report/flag/context/UOW policy or initialized values.
//! Sources: original MQGET/MQPUT/MQPUT1; pinned supplemental q097390_/q091870_
//! and layout scalar/structure declarations. See ADR 0033-mq-full-md-value.

/// Trusted structure character interpretation, never inferred from body CCSID.
/// Neither profile promises UTF-8 or normalizes blanks, nulls or arbitrary bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMdCharacterEncoding {
    /// The existing raw structure's ASCII-compatible single-byte profile.
    AsciiCompatible,
    /// The existing owned CP037 structure profile, not an MQ-native assertion.
    OwnedCp037,
}

/// Every common field except Version, represented by the enclosing variant.
/// Arrays are exact MQCHAR/MQBYTE widths, including significant padding/nulls.
/// All numeric observations are signed 32-bit MQLONG, including unknown values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMdFields {
    /// Exact MQCHAR4 structure identifier, in the declared structure profile.
    pub struc_id: [u8; 4],
    /// Observed report options; permitted combinations remain per-call policy.
    pub report: i32,
    /// Observed message type, without numeric admission.
    pub msg_type: i32,
    /// Observed MQ expiry units, without host-tick conversion.
    pub expiry: i32,
    /// Observed feedback or reason number.
    pub feedback: i32,
    /// Numeric encoding of the BODY, never the structure's decoder selector.
    pub encoding: i32,
    /// Character set of the BODY, never the fixed-character array interpretation.
    pub coded_char_set_id: i32,
    /// Exact MQCHAR8 body format name.
    pub format: [u8; 8],
    /// Observed priority or sentinel, without queue-default resolution.
    pub priority: i32,
    /// Observed persistence or sentinel, without default resolution.
    pub persistence: i32,
    /// Exact MQBYTE24 message identifier, not executable ownership.
    pub msg_id: [u8; 24],
    /// Exact MQBYTE24 correlation identifier.
    pub correl_id: [u8; 24],
    /// Observed signed backout counter.
    pub backout_count: i32,
    /// Exact MQCHAR48 reply queue, retaining blanks and nulls.
    pub reply_to_q: [u8; 48],
    /// Exact MQCHAR48 reply queue manager.
    pub reply_to_q_mgr: [u8; 48],
    /// Exact MQCHAR12 user identity context, not a SAF principal assertion.
    pub user_identifier: [u8; 12],
    /// Exact MQBYTE32 accounting token.
    pub accounting_token: [u8; 32],
    /// Exact MQCHAR32 application identity context.
    pub appl_identity_data: [u8; 32],
    /// Observed application type, without permitted-context admission.
    pub put_appl_type: i32,
    /// Exact MQCHAR28 application name context.
    pub put_appl_name: [u8; 28],
    /// Exact MQCHAR8 put date; no parsing or normalization.
    pub put_date: [u8; 8],
    /// Exact MQCHAR8 put time; no parsing or normalization.
    pub put_time: [u8; 8],
    /// Exact MQCHAR4 application origin context.
    pub appl_origin_data: [u8; 4],
}

/// Exactly the five MQMD2-only fields. MQMD1 never invents these observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMdV2Fields {
    /// Exact MQBYTE24 group identifier; absent from version 1.
    pub group_id: [u8; 24],
    /// Observed signed logical message sequence.
    pub msg_seq_number: i32,
    /// Observed signed segment offset.
    pub offset: i32,
    /// All observed flag bits, without legality or generation permission.
    pub msg_flags: i32,
    /// Observed signed original length, including sentinels.
    pub original_length: i32,
}

/// Complete pointer-free descriptor VALUE, not a message/effect/storage schema.
/// Raw byte order/capacity/suffix remain source-capture observations. MQLONG
/// values canonically ignore physical byte order; character bytes do not.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMdValue {
    /// Complete version-one descriptor, with no fabricated v2 fields.
    V1 {
        /// Trusted interpretation for all fixed character arrays.
        characters: MqMdCharacterEncoding,
        /// All version-one field observations except the variant's Version.
        fields: MqMdFields,
    },
    /// Complete version-two descriptor and its exact extension fields.
    V2 {
        /// Trusted interpretation for all fixed character arrays.
        characters: MqMdCharacterEncoding,
        /// All common field observations except the variant's Version.
        fields: MqMdFields,
        /// Actual five additional observations, never default-filled.
        extension: MqMdV2Fields,
    },
}

/// Representation/codec failure; no variant admits a per-call semantic policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMdValueProblem {
    /// StrucId does not match the declared structure character profile.
    StructureIdentifier,
    /// The codec variant is not the frozen version-one/two vocabulary.
    UnsupportedVersion,
    /// The codec character profile is not one of the explicit trusted profiles.
    UnsupportedCharacterEncoding,
    /// Incorrect schema, type, field, order, count, tag, width or trailing data.
    Malformed,
    /// Required bytes are absent.
    Truncated,
    /// Explicit caller or product byte ceiling exceeded.
    CanonicalLimit,
    /// Bounded output allocation could not be reserved.
    Allocation,
}

impl MqMdValue {
    /// The exact observed MQMD Version; later versions are not this vocabulary.
    pub const fn version(&self) -> i32 {
        match self {
            Self::V1 { .. } => 1,
            Self::V2 { .. } => 2,
        }
    }
    /// Exact interpretation of fixed-character bytes, distinct from body CCSID.
    pub const fn characters(&self) -> MqMdCharacterEncoding {
        match self {
            Self::V1 { characters, .. } | Self::V2 { characters, .. } => *characters,
        }
    }
    /// Borrow every common observation, without loss or per-call validation.
    pub const fn fields(&self) -> &MqMdFields {
        match self {
            Self::V1 { fields, .. } | Self::V2 { fields, .. } => fields,
        }
    }
    /// Checks only exact structure identity. Width/version/type are encoded by
    /// this value's Rust types. All other numeric/character observations remain
    /// exact even when unknown or inadmissible to a future MQGET/PUT operation.
    /// This is NOT report/flag/expiry/priority/context authorization or validation.
    pub fn validate_representation(&self) -> Result<(), MqMdValueProblem> {
        crate::mq_raw_layout::validate_md_identity(self)
    }
}

#[cfg(test)]
pub(crate) mod tests;
