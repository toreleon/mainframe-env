//! Explicit native metadata in the same catalog; source presence is not admission.
use super::*;
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use serde::{Deserialize, Serialize};
#[cfg(test)]
mod tests;

/// Additive catalog format; @1 remains unchanged and contains no native defaults.
pub const MQ_OBJECT_NATIVE_CATALOG_SCHEMA: &str = "mainframe-env.mq-object-catalog@2";
/// Explicit producer defaults; older catalog encodings remain unchanged.
pub const MQ_OBJECT_PRODUCER_CATALOG_SCHEMA: &str = "mainframe-env.mq-object-catalog@3";

/// Actual configured queue persistence, with no pending/default interpretation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MqNativePersistence {
    /// MQPER_NOT_PERSISTENT; cold restart does not retain the message.
    NonPersistent,
    /// MQPER_PERSISTENT; persistence applies once the put is committed, including
    /// an immediate no-syncpoint put, not an uncommitted local-unit candidate.
    Persistent,
}
/// Explicit configured put response, distinct from successful publication.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MqNativePutResponse {
    /// Complete synchronous producer output is represented by this profile.
    Synchronous,
    /// Retained configuration; default asynchronous execution remains pending.
    Asynchronous,
}
/// q103140_/q103180_/q103190_: source-defined attributes in the sole catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MqNativeProducerDefaults {
    /// Effective priority at put time, zero through the actual QM MaxPriority.
    pub priority: i32,
    /// Effective message persistence at put time; not application MD writeback.
    pub persistence: MqNativePersistence,
    /// Captured ordinary queue default; no client override or async receipt claim.
    pub response: MqNativePutResponse,
}
/// Explicit supported fixed-character policy, distinct from body CCSID.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MqNativeCharacters {
    /// Explicit CCSID819 single-byte profile, not an IBM z/OS startup default.
    Ascii819,
    /// Explicit owned CCSID37 profile, not an IBM z/OS startup default500.
    OwnedCp037,
}
impl MqNativeCharacters {
    pub(crate) fn md(self) -> MqMdCharacterEncoding {
        match self {
            Self::Ascii819 => MqMdCharacterEncoding::AsciiCompatible,
            Self::OwnedCp037 => MqMdCharacterEncoding::OwnedCp037,
        }
    }
}
/// Source-defined delivery ordering observation; producer execution is narrower.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MqNativeDeliverySequence {
    /// FIFO arrival ordering, independently of the message's effective priority.
    Fifo,
    /// Priority ordering is retained metadata, pending this producer profile.
    Priority,
}
/// Explicit per-normal-local-queue metadata, never a legacy global default.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MqNativeQueueAttributes {
    /// Exact predefined local normal queue identity.
    pub name: MqObjectName,
    /// q103280_: finite zero through104857600; zero permits only empty body.
    pub max_msg_length: i32,
    /// q103300_: exact configured delivery ordering.
    pub delivery_sequence: MqNativeDeliverySequence,
    /// Explicit defaults select catalog@3. Absence preserves exact catalog@2
    /// bytes and refuses default policy rather than supplying startup guesses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_defaults: Option<MqNativeProducerDefaults>,
}
/// Native metadata extension of the sole MqObjectCatalog, installed explicitly.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MqNativeAttributes {
    /// q102230_: explicit actual QM structure CCSID; body CCSID is independent.
    pub coded_char_set_id: i32,
    /// Checked explicit structure-character support paired with the actual CCSID.
    pub characters: MqNativeCharacters,
    /// q102510_: actual QM maximum,32768 through104857600.
    pub max_msg_length: i32,
    /// q102520_: nonnegative maximum; no invented upper bound is imposed.
    pub max_priority: i32,
    /// Exactly one sorted entry for each predefined normal local queue.
    pub queues: Vec<MqNativeQueueAttributes>,
}
impl MqObjectCatalog {
    /// Explicit value/configuration construction before installation. Does not
    /// authorize deployed replacement, migrate rows or choose runtime defaults.
    pub fn with_native_attributes(
        mut self,
        attributes: MqNativeAttributes,
    ) -> Result<Self, MqObjectError> {
        self.validate_native_attributes(&attributes)?;
        self.native_attributes = Some(attributes);
        self.encode()?;
        Ok(self)
    }
    /// Same catalog's explicit metadata, absent for historical catalog@1.
    pub fn native_attributes(&self) -> Option<&MqNativeAttributes> {
        self.native_attributes.as_ref()
    }
    pub(super) fn validate_native_attributes(
        &self,
        attrs: &MqNativeAttributes,
    ) -> Result<(), MqObjectError> {
        let ccsid = match attrs.characters {
            MqNativeCharacters::Ascii819 => 819,
            MqNativeCharacters::OwnedCp037 => 37,
        };
        if attrs.coded_char_set_id != ccsid
            || !(32768..=104857600).contains(&attrs.max_msg_length)
            || attrs.max_priority < 0
            || attrs.queues.len() > self.limits.max_objects
            || attrs.queues.windows(2).any(|w| w[0].name >= w[1].name)
        {
            return Err(MqObjectError::CorruptSnapshot);
        }
        let normal: Vec<_> = self
            .definitions()
            .filter_map(|d| match d {
                MqObjectDefinition::LocalQueue {
                    name,
                    usage: MqLocalQueueUsage::Normal,
                    ..
                } => Some(name),
                _ => None,
            })
            .collect();
        if normal.len() != attrs.queues.len()
            || normal
                .iter()
                .zip(&attrs.queues)
                .any(|(name, a)| **name != a.name || !(0..=104857600).contains(&a.max_msg_length))
        {
            return Err(MqObjectError::InvalidReferenceKind);
        }
        if attrs.queues.iter().any(|q| {
            q.producer_defaults
                .as_ref()
                .is_some_and(|d| d.priority < 0 || d.priority > attrs.max_priority)
        }) {
            return Err(MqObjectError::CorruptSnapshot);
        }
        Ok(())
    }
}
