//! First checked property profile, distinct from retained private ContractDefault.
//! The ONE structure catalog generates numeric/default/descriptor facts. Source
//! review is not live host, registry, SAF, UOW or mutation permission.

use super::*;
use crate::MqPropertyType;
use crate::mq_md_value::MqMdValue;
mod observation;
mod request;
pub(super) use observation::mq_property_bind;
pub use observation::{MqPropertyInquiryObservation, MqPropertyObservation};
pub use request::MqPropertyRequest;

mod generated {
    use super::*;
    include!("property/generated.rs");
}
pub use generated::MQ_PROPERTY_PROFILE_SHA256;

/// One reviewed numeric identity; membership alone admits no option combination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqPropertyNumericFact {
    /// IBM symbol, with ZOS qualification for platform-dependent MQENC_NATIVE.
    pub symbol: &'static str,
    /// Exact signed MQLONG value, never inferred from an application's topology.
    pub value: i32,
}
/// Reviewed constants including recognized forms still unsupported by this profile.
pub fn mq_property_numeric_identities() -> &'static [MqPropertyNumericFact] {
    generated::NUMERIC_IDENTITIES
}
/// Explicit UTF-8 profile reviewed from the pinned MQCHARV CCSID example.
/// APPL/current-process resolution remains outside this typed constructor.
pub fn mq_property_profile_ccsid() -> i32 {
    generated::PROPERTY_UTF8_CCSID
}

/// Known refusal at the checked property boundary; no runtime MQ outcome calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPropertyProblem {
    /// Not one of the five property call identities.
    Call,
    /// Unrecognized structure version or option bits.
    Options,
    /// Known but unrepresented encoding, conversion, cursor, context or lifetime form.
    Unsupported,
    /// Invalid or out-of-profile property name; bytes are never normalized.
    Name,
    /// Type/width/value/descriptor mismatch or an out-of-domain signed scalar.
    Value,
    /// Product capacity/aggregate bound, not an IBM resource quota.
    Capacity,
}

/// Source-defined default version/option input, bound to its exact call family.
/// Private fields prevent arbitrary integers from bypassing the checked constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqPropertyOptions {
    pub(super) call: MqMqiCall,
    pub(super) version: i32,
    pub(super) options: i32,
}
impl MqPropertyOptions {
    /// Admits only version1/default behavior. Known nondefault modes stay
    /// Unsupported; negative/unknown bits fail. It selects no host or authority.
    pub fn checked(call: MqMqiCall, version: i32, options: i32) -> Result<Self, MqPropertyProblem> {
        use generated::*;
        let (expected, default, known) = match call {
            MqMqiCall::CreateMessageHandle => (
                MQCMHO_VERSION_1,
                MQCMHO_DEFAULT_VALIDATION,
                MQCMHO_NO_VALIDATION | MQCMHO_VALIDATE,
            ),
            MqMqiCall::DeleteMessageHandle => (MQDMHO_VERSION_1, MQDMHO_NONE, MQDMHO_NONE),
            MqMqiCall::DeleteProperty => (
                MQDMPO_VERSION_1,
                MQDMPO_DEL_FIRST,
                MQDMPO_DEL_PROP_UNDER_CURSOR,
            ),
            MqMqiCall::InquireProperty => (
                MQIMPO_VERSION_1,
                MQIMPO_INQ_FIRST,
                MQIMPO_CONVERT_TYPE
                    | MQIMPO_QUERY_LENGTH
                    | MQIMPO_INQ_NEXT
                    | MQIMPO_INQ_PROP_UNDER_CURSOR
                    | MQIMPO_CONVERT_VALUE,
            ),
            MqMqiCall::SetProperty => (
                MQSMPO_VERSION_1,
                MQSMPO_SET_FIRST,
                MQSMPO_SET_PROP_UNDER_CURSOR
                    | MQSMPO_SET_PROP_AFTER_CURSOR
                    | MQSMPO_APPEND_PROPERTY
                    | MQSMPO_SET_PROP_BEFORE_CURSOR,
            ),
            _ => return Err(MqPropertyProblem::Call),
        };
        if version != expected {
            return Err(MqPropertyProblem::Unsupported);
        }
        if options < 0 || options & !known != 0 {
            return Err(MqPropertyProblem::Options);
        }
        if options != default {
            return Err(MqPropertyProblem::Unsupported);
        }
        Ok(Self {
            call,
            version,
            options,
        })
    }
    /// Exact call family, not a dispatch registration.
    pub fn call(self) -> MqMqiCall {
        self.call
    }
    /// Generated structure identity for this finite option family. This is a
    /// typed value, not a raw pointer or an implicit initializer for missing input.
    pub fn structure_id(self) -> [u8; 4] {
        match self.call {
            MqMqiCall::CreateMessageHandle => generated::MQCMHO_STRUC_ID,
            MqMqiCall::DeleteMessageHandle => generated::MQDMHO_STRUC_ID,
            MqMqiCall::DeleteProperty => generated::MQDMPO_STRUC_ID,
            MqMqiCall::InquireProperty => generated::MQIMPO_STRUC_ID,
            MqMqiCall::SetProperty => generated::MQSMPO_STRUC_ID,
            _ => unreachable!("checked property family"),
        }
    }
}

/// Full MQPD1 input/output observations in this ASCII-compatible typed profile.
/// Explicit symbolic constants define optional support/default copy as1/22;
/// the pinned structure table's inconsistent zero columns are not admitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqPropertyDescriptor {
    /// Exact four-character MQPD identifier, including its two blanks.
    pub struc_id: [u8; 4],
    /// Reviewed MQPD version; newer versions are not this value contract.
    pub version: i32,
    /// Exact option bits; only MQPD_NONE is represented.
    pub options: i32,
    /// Exact support classification; ordinary properties are optional.
    pub support: i32,
    /// Exact context classification; this profile supplies no user-context authority.
    pub context: i32,
    /// Exact copy classification; descriptor properties return MQCOPY_NONE.
    pub copy_options: i32,
}
impl MqPropertyDescriptor {
    /// Generated symbolic default MQPD1; no missing field is default-filled.
    pub fn source_default() -> Self {
        use generated::*;
        Self {
            struc_id: MQPD_STRUC_ID,
            version: MQPD_VERSION_1,
            options: MQPD_NONE,
            support: MQPD_SUPPORT_OPTIONAL,
            context: MQPD_NO_CONTEXT,
            copy_options: MQCOPY_DEFAULT,
        }
    }
    /// Complete finite input validation; integer membership is not permission.
    pub fn validate_input(&self) -> Result<(), MqPropertyProblem> {
        if *self != Self::source_default() {
            return Err(MqPropertyProblem::Unsupported);
        }
        Ok(())
    }
    /// Defined descriptor-property output copy classification, otherwise identical.
    pub fn descriptor_output() -> Self {
        Self {
            copy_options: generated::MQCOPY_NONE,
            ..Self::source_default()
        }
    }
}

/// Exact bounded name; no synonym, Unicode conversion, wildcard or pointer mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqPropertyName(pub(super) String);
impl MqPropertyName {
    /// Checks the finite ASCII Java-identifier/dotted hierarchy subset. Reserved
    /// namespaces and aliases remain unsupported except exact Root.MQMD.Field.
    pub fn checked(value: String, limits: MqMessageLimits) -> Result<Self, MqPropertyProblem> {
        limits.validate().map_err(|_| MqPropertyProblem::Capacity)?;
        if value.is_empty() || value.len() > limits.property_name_bytes {
            return Err(MqPropertyProblem::Capacity);
        }
        if !value.is_ascii() {
            return Err(MqPropertyProblem::Unsupported);
        }
        for part in value.split('.') {
            let mut bytes = part.bytes();
            if !bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'$')
                || bytes.any(|b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'$')
            {
                return Err(MqPropertyProblem::Name);
            }
        }
        let upper = value.to_ascii_uppercase();
        if [
            "NULL", "TRUE", "FALSE", "NOT", "AND", "OR", "BETWEEN", "LIKE", "IN", "IS", "ESCAPE",
        ]
        .contains(&upper.as_str())
        {
            return Err(MqPropertyProblem::Name);
        }
        if let Some(field) = value.strip_prefix("Root.MQMD.") {
            if !generated::MD_FIELDS.iter().any(|f| f.name == field) {
                return Err(MqPropertyProblem::Unsupported);
            }
        } else {
            let lower = value.to_ascii_lowercase();
            if [
                "jms",
                "usr",
                "mq",
                "mcd",
                "sib",
                "wmq",
                "root",
                "body",
                "properties",
            ]
            .iter()
            .any(|p| lower.starts_with(p))
            {
                return Err(MqPropertyProblem::Unsupported);
            }
        }
        Ok(Self(value))
    }
    /// Original exact name, without trimming, folding or namespace rewriting.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Exact associated-descriptor field, or ordinary property classification.
    pub fn descriptor_field(&self) -> Option<&str> {
        self.0.strip_prefix("Root.MQMD.")
    }
}

/// Entire AS_SET value, retaining numeric encoding and explicit string CCSID.
/// Value bytes have no Serde/live control authority; the request validates bounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqPropertyData {
    /// Exact null/bytes/string/signed integer type; floats/conversion remain pending.
    pub kind: MqPropertyType,
    /// Source z/OS native785, never simulator-native structure byte order.
    pub encoding: i32,
    /// Explicit UTF-8 CCSID1208; application sentinel resolution is not inferred.
    pub ccsid: i32,
    /// Exact complete value, including embedded null/high bytes and significant blanks.
    pub bytes: Vec<u8>,
}
impl MqPropertyData {
    /// Validate the finite as-set profile without selecting execution or conversion.
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqPropertyProblem> {
        if self.encoding != generated::MQENC_NATIVE_ZOS
            || self.ccsid != generated::PROPERTY_UTF8_CCSID
        {
            return Err(MqPropertyProblem::Unsupported);
        }
        if self.bytes.len() > limits.property_value_bytes || self.bytes.len() > i32::MAX as usize {
            return Err(MqPropertyProblem::Capacity);
        }
        if matches!(
            self.kind,
            MqPropertyType::Boolean | MqPropertyType::Float32 | MqPropertyType::Float64
        ) {
            return Err(MqPropertyProblem::Unsupported);
        }
        let width = generated::value_width(self.kind);
        if width.is_some_and(|w| self.bytes.len() != w) {
            return Err(MqPropertyProblem::Value);
        }
        if self.kind == MqPropertyType::String && std::str::from_utf8(&self.bytes).is_err() {
            return Err(MqPropertyProblem::Value);
        }
        Ok(())
    }
    /// Checked MQTYPE numeric translation; aliases INT32/LONG share the reviewed
    /// exact64 identity. Unknown/float/Boolean/AS_SET input values fail closed.
    pub fn from_numeric(
        kind: i32,
        encoding: i32,
        ccsid: i32,
        bytes: Vec<u8>,
        limits: MqMessageLimits,
    ) -> Result<Self, MqPropertyProblem> {
        use generated::*;
        let kind = match kind {
            MQTYPE_NULL => MqPropertyType::Null,
            MQTYPE_BYTE_STRING => MqPropertyType::ByteString,
            MQTYPE_INT8 => MqPropertyType::Int8,
            MQTYPE_INT16 => MqPropertyType::Int16,
            MQTYPE_INT32 => MqPropertyType::Int32,
            MQTYPE_INT64 => MqPropertyType::Int64,
            MQTYPE_STRING => MqPropertyType::String,
            _ => return Err(MqPropertyProblem::Unsupported),
        };
        let value = Self {
            kind,
            encoding,
            ccsid,
            bytes,
        };
        value.validate(limits)?;
        Ok(value)
    }
}

/// Generated descriptor field kind/width, not per-call scalar or context permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqPropertyMdField {
    /// Exact C-declaration case used by Root.MQMD.Field.
    pub name: &'static str,
    /// Corresponding represented property data type.
    pub kind: MqPropertyType,
    /// Complete fixed bytes; callers must not trim or invent padding.
    pub width: usize,
}
/// Complete version-one associated descriptor field vocabulary, excluding ID/version.
pub fn mq_property_md_fields() -> &'static [MqPropertyMdField] {
    generated::MD_FIELDS
}
/// Source-derived complete associated MQMD1 initializer in the owned typed
/// ASCII-compatible profile. This is not an IBM native structure codec.
pub fn mq_property_initial_descriptor() -> MqMdValue {
    generated::initial_md()
}
/// Exact field observation, preserving complete fixed arrays and signed numeric bytes.
pub fn mq_property_descriptor_bytes(md: &MqMdValue, field: &str) -> Option<Vec<u8>> {
    generated::md_bytes(md, field)
}
/// Checked complete field replacement only. Scalar legality/policy remains with
/// the source-reviewed request validator, not this representation helper.
pub fn mq_property_set_descriptor_bytes(
    md: &mut MqMdValue,
    field: &str,
    bytes: &[u8],
) -> Result<(), MqPropertyProblem> {
    generated::set_md_bytes(md, field, bytes)
}

#[cfg(test)]
mod tests;
