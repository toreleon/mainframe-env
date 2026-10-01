//! Bounded MQ message vocabulary for later delivery, pub/sub, and recovery work.
//!
//! This is a host contract, not an MQI handler or an MQMD wire layout. Product
//! ceilings below are resource guards, not IBM numeric constants. Exact MQMD
//! ranges, encoding, selector legality, and delivery behavior remain pending.

/// Contract identity; no executable MQI coverage follows from this module.
pub const MQ_MESSAGE_CONTRACT: &str = "mainframe-env.mq-message@1";

/// Pinned IBM MQ 9.4 call-topic identities reviewed for this vocabulary.
/// Rows refer to `conformance/0.2/catalogs/mq.json`, baseline
/// `ibm-mq-9.4-mqi-2026-08-31`; hashes refer to its topic manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMessageSource {
    pub row: &'static str,
    pub call: &'static str,
    pub topic_path: &'static str,
    pub topic_sha256: &'static str,
}

pub const MQ_MESSAGE_SOURCES: &[MqMessageSource] = &[
    MqMessageSource {
        row: "0003",
        call: "MQBUFMH",
        topic_path: "SSFKSJ_9.4.0/refdev/q101710_.html",
        topic_sha256: "8a94879a9c9f2e18ddb2171b0dd5ea477ebaf26684a760c9e1e31931f2084156",
    },
    MqMessageSource {
        row: "0010",
        call: "MQCRTMH",
        topic_path: "SSFKSJ_9.4.0/refdev/q101780_.html",
        topic_sha256: "66cf482573408e227aec23ec2219cd52bf7bf879591f33acb04dc3f33fb386db",
    },
    MqMessageSource {
        row: "0013",
        call: "MQDLTMH",
        topic_path: "SSFKSJ_9.4.0/refdev/q101810_.html",
        topic_sha256: "dc85a4c2b9e2e552615a5e539e3d3066682170f812f2e5f25d8f2346d9f8254e",
    },
    MqMessageSource {
        row: "0014",
        call: "MQDLTMP",
        topic_path: "SSFKSJ_9.4.0/refdev/q101820_.html",
        topic_sha256: "84dee0f8d9659e978f8cadcaaac57dfb7b3697d2b7be949121fb085d9240894d",
    },
    MqMessageSource {
        row: "0015",
        call: "MQGET",
        topic_path: "SSFKSJ_9.4.0/refdev/q101830_.html",
        topic_sha256: "290b8af3acbe4a87f007ab9e3b67d0a797f835066118c9c6150ff0570e430b62",
    },
    MqMessageSource {
        row: "0017",
        call: "MQINQMP",
        topic_path: "SSFKSJ_9.4.0/refdev/q101850_.html",
        topic_sha256: "46d73ea328544e00e2f84d07de4e9243c691609f76e2f54c5f6cb6377197be2b",
    },
    MqMessageSource {
        row: "0018",
        call: "MQMHBUF",
        topic_path: "SSFKSJ_9.4.0/refdev/q101860_.html",
        topic_sha256: "79cca3ee757d01445b79275216431b6146bfdd7aa9f9e2a24b9532ed13ba6b30",
    },
    MqMessageSource {
        row: "0020",
        call: "MQPUT",
        topic_path: "SSFKSJ_9.4.0/refdev/q101880_.html",
        topic_sha256: "47f73a96d6926573dd0a09a33a2e48d8389aedef7a2ded58d5d3e7562f393aab",
    },
    MqMessageSource {
        row: "0021",
        call: "MQPUT1",
        topic_path: "SSFKSJ_9.4.0/refdev/q101890_.html",
        topic_sha256: "6b51813a2e99c74f04c22c599d4ad82d0b0eec31924abb6dc8e03e1ed7438dcb",
    },
    MqMessageSource {
        row: "0023",
        call: "MQSETMP",
        topic_path: "SSFKSJ_9.4.0/refdev/q101910_.html",
        topic_sha256: "5c1eddf9f87db568f941fcb283eb1e64c724eae5e64cb56a20f134a481a27a30",
    },
    MqMessageSource {
        row: "0024",
        call: "MQSTAT",
        topic_path: "SSFKSJ_9.4.0/refdev/q101920_.html",
        topic_sha256: "4f19dab47ed3e325ec894cc74a8d88db265a7c27f8ff2a7c507f94cf4bedd1d9",
    },
];

/// Product resource ceilings, independent of queue and queue-manager limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMessageLimits {
    pub body_bytes: usize,
    pub identifier_bytes: usize,
    pub format_bytes: usize,
    pub properties: usize,
    pub property_name_bytes: usize,
    pub property_value_bytes: usize,
    pub property_total_bytes: usize,
    pub distribution_items: usize,
    pub destination_bytes: usize,
    pub wait_ticks: u64,
}

impl Default for MqMessageLimits {
    fn default() -> Self {
        Self {
            body_bytes: 1024 * 1024, // existing MQ service default
            identifier_bytes: 64,
            format_bytes: 32,
            properties: 128,
            property_name_bytes: 256,
            property_value_bytes: 64 * 1024,
            property_total_bytes: 1024 * 1024,
            distribution_items: 256,
            destination_bytes: 256,
            wait_ticks: 1_000_000,
        }
    }
}

impl MqMessageLimits {
    /// Prevents a caller from widening the frozen product ceiling.
    pub fn validate(self) -> Result<(), MqMessageProblem> {
        let ceiling = Self::default();
        let fields = [
            (self.body_bytes, ceiling.body_bytes),
            (self.identifier_bytes, ceiling.identifier_bytes),
            (self.format_bytes, ceiling.format_bytes),
            (self.properties, ceiling.properties),
            (self.property_name_bytes, ceiling.property_name_bytes),
            (self.property_value_bytes, ceiling.property_value_bytes),
            (self.property_total_bytes, ceiling.property_total_bytes),
            (self.distribution_items, ceiling.distribution_items),
            (self.destination_bytes, ceiling.destination_bytes),
        ];
        if fields
            .iter()
            .any(|(value, maximum)| *value == 0 || value > maximum)
            || self.wait_ticks == 0
            || self.wait_ticks > ceiling.wait_ticks
        {
            return Err(MqMessageProblem::Limits);
        }
        Ok(())
    }
}

/// Parts of IBM-observable semantics requiring separately pinned structures or
/// later execution evidence. Consumers must not treat this as a support list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMessagePending {
    DescriptorNumericLegality,
    DescriptorEncoding,
    ExpiryUnitMapping,
    PropertyNameRules,
    PropertyEncodingAndConversion,
    SelectorOptionLegality,
    GroupAndSegmentFlagMapping,
    DistributionCompletionMapping,
    DeliveryAndRecovery,
}

pub const MQ_MESSAGE_PENDING: &[MqMessagePending] = &[
    MqMessagePending::DescriptorNumericLegality,
    MqMessagePending::DescriptorEncoding,
    MqMessagePending::ExpiryUnitMapping,
    MqMessagePending::PropertyNameRules,
    MqMessagePending::PropertyEncodingAndConversion,
    MqMessagePending::SelectorOptionLegality,
    MqMessagePending::GroupAndSegmentFlagMapping,
    MqMessagePending::DistributionCompletionMapping,
    MqMessagePending::DeliveryAndRecovery,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMessageProblem {
    Limits,
    BodyTooLong,
    Identifier,
    Format,
    Expiry,
    PropertyCount,
    PropertyName,
    PropertyValueLength,
    PropertyTotalLength,
    DuplicateProperty,
    Group,
    Segment,
    Distribution,
    Wait,
    Cursor,
    Truncation,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MqMessageIdentifiers {
    pub message_id: Option<Vec<u8>>,
    pub correlation_id: Option<Vec<u8>>,
    pub group_id: Option<Vec<u8>>,
}

impl MqMessageIdentifiers {
    fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMessageProblem> {
        if [
            self.message_id.as_deref(),
            self.correlation_id.as_deref(),
            self.group_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|id| id.is_empty() || id.len() > limits.identifier_bytes)
        {
            return Err(MqMessageProblem::Identifier);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqExpiry {
    Unlimited,
    /// Host logical ticks; IBM MQ expiry units still require a pinned mapping.
    RelativeHostTicks(u64),
    PendingSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPersistence {
    QueueDefault,
    Persistent,
    NonPersistent,
    PendingSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPriority {
    QueueDefault,
    /// Raw requested value with IBM range validation pending.
    PendingNumeric(i32),
}

/// Source-visible MQMD group/segment concepts; numeric flags remain pending.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MqMessageOrdering {
    pub group_sequence: Option<u32>,
    pub last_in_group: bool,
    pub segment_offset: Option<u64>,
    pub last_segment: bool,
    pub segmentation_allowed: bool,
}

impl MqMessageOrdering {
    fn validate(
        &self,
        group_id: Option<&[u8]>,
        limits: MqMessageLimits,
        body_len: usize,
    ) -> Result<(), MqMessageProblem> {
        if group_id.is_some() != self.group_sequence.is_some()
            || self.group_sequence == Some(0)
            || (self.last_in_group && group_id.is_none())
        {
            return Err(MqMessageProblem::Group);
        }
        if (self.last_segment && self.segment_offset.is_none())
            || self.segment_offset.is_some_and(|offset| {
                offset
                    .checked_add(body_len as u64)
                    .is_none_or(|end| end > limits.body_bytes as u64)
            })
        {
            return Err(MqMessageProblem::Segment);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMessageDescriptor {
    pub identifiers: MqMessageIdentifiers,
    /// Opaque MQMD-like format name; exact IBM field layout is pending.
    pub format: Option<String>,
    pub expiry: MqExpiry,
    pub persistence: MqPersistence,
    pub priority: MqPriority,
    pub ordering: MqMessageOrdering,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqPropertyType {
    Boolean,
    ByteString,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    String,
    Null,
}

impl MqPropertyType {
    fn expected_bytes(self) -> Option<usize> {
        match self {
            Self::Boolean | Self::Int32 | Self::Float32 => Some(4),
            Self::Int8 => Some(1),
            Self::Int16 => Some(2),
            Self::Int64 | Self::Float64 => Some(8),
            Self::Null => Some(0),
            Self::ByteString | Self::String => None,
        }
    }
}

/// MQSETMP-style typed value bytes. Encoding and conversion stay pending.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMessageProperty {
    pub name: String,
    pub kind: MqPropertyType,
    pub value: Vec<u8>,
}

impl MqMessageProperty {
    fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMessageProblem> {
        validate_name(&self.name, limits.property_name_bytes, false)
            .map_err(|_| MqMessageProblem::PropertyName)?;
        if self.value.len() > limits.property_value_bytes
            || self
                .kind
                .expected_bytes()
                .is_some_and(|size| self.value.len() != size)
        {
            return Err(MqMessageProblem::PropertyValueLength);
        }
        Ok(())
    }
}

/// Exact name for set/delete; prefix represents the terminal `%` wildcard
/// documented for inquire and message-handle-to-buffer operations only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqPropertyQuery {
    Exact(String),
    Prefix(String),
}

impl MqPropertyQuery {
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMessageProblem> {
        limits.validate()?;
        let (name, allow_empty) = match self {
            Self::Exact(name) => (name.as_str(), false),
            Self::Prefix(prefix) => (prefix.as_str(), true),
        };
        validate_name(name, limits.property_name_bytes, allow_empty)
            .map_err(|_| MqMessageProblem::PropertyName)
    }
}

fn validate_name(name: &str, maximum: usize, allow_empty: bool) -> Result<(), ()> {
    if (!allow_empty && name.is_empty())
        || name.len() > maximum
        || name.bytes().any(|byte| byte == 0 || byte == b'%')
    {
        return Err(());
    }
    Ok(())
}

/// A bounded value container, reusable by put, get, publish, and replay lanes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMessage {
    pub descriptor: MqMessageDescriptor,
    pub body: Vec<u8>,
    pub properties: Vec<MqMessageProperty>,
}

impl MqMessage {
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMessageProblem> {
        limits.validate()?;
        if self.body.len() > limits.body_bytes {
            return Err(MqMessageProblem::BodyTooLong);
        }
        self.descriptor.identifiers.validate(limits)?;
        self.descriptor.ordering.validate(
            self.descriptor.identifiers.group_id.as_deref(),
            limits,
            self.body.len(),
        )?;
        if self.descriptor.format.as_ref().is_some_and(|format| {
            format.is_empty() || format.len() > limits.format_bytes || format.contains('\0')
        }) {
            return Err(MqMessageProblem::Format);
        }
        if self.descriptor.expiry == MqExpiry::RelativeHostTicks(0) {
            return Err(MqMessageProblem::Expiry);
        }
        if self.properties.len() > limits.properties {
            return Err(MqMessageProblem::PropertyCount);
        }
        let mut names = std::collections::BTreeSet::new();
        let mut total = 0usize;
        for property in &self.properties {
            property.validate(limits)?;
            if !names.insert(property.name.as_str()) {
                return Err(MqMessageProblem::DuplicateProperty);
            }
            total = total
                .checked_add(property.name.len())
                .and_then(|size| size.checked_add(property.value.len()))
                .ok_or(MqMessageProblem::PropertyTotalLength)?;
            if total > limits.property_total_bytes {
                return Err(MqMessageProblem::PropertyTotalLength);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MqMessageMatch {
    pub identifiers: MqMessageIdentifiers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqGetMode {
    Remove,
    BrowseFirst,
    BrowseNext { cursor: u64 },
    RemoveUnderCursor { cursor: u64 },
}

impl MqGetMode {
    fn removes_message(self) -> bool {
        matches!(self, Self::Remove | Self::RemoveUnderCursor { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqWait {
    NoWait,
    BoundedHostTicks(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqTruncation {
    Reject,
    Accept,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqGetContract {
    pub selection: MqMessageMatch,
    pub mode: MqGetMode,
    pub wait: MqWait,
    pub truncation: MqTruncation,
    pub buffer_capacity: usize,
}

impl MqGetContract {
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMessageProblem> {
        limits.validate()?;
        self.selection.identifiers.validate(limits)?;
        if self.buffer_capacity > limits.body_bytes {
            return Err(MqMessageProblem::BodyTooLong);
        }
        if matches!(self.wait, MqWait::BoundedHostTicks(0))
            || matches!(self.wait, MqWait::BoundedHostTicks(ticks) if ticks > limits.wait_ticks)
        {
            return Err(MqMessageProblem::Wait);
        }
        if matches!(
            self.mode,
            MqGetMode::BrowseNext { cursor: 0 } | MqGetMode::RemoveUnderCursor { cursor: 0 }
        ) {
            return Err(MqMessageProblem::Cursor);
        }
        Ok(())
    }
}

/// A reported get result retains both actual and copied lengths. The delivery
/// lane determines whether a truncated message remains on the queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqTruncationDisposition {
    Complete { length: usize },
    RejectedRetained { required: usize, copied: usize },
    AcceptedRemoved { required: usize, copied: usize },
    AcceptedBrowsed { required: usize, copied: usize },
}

impl MqTruncationDisposition {
    pub fn validate(self, request: &MqGetContract) -> Result<(), MqMessageProblem> {
        let capacity = request.buffer_capacity;
        match self {
            Self::Complete { length } if length <= capacity => Ok(()),
            Self::RejectedRetained { required, copied }
                if request.truncation == MqTruncation::Reject
                    && required > capacity
                    && copied <= capacity =>
            {
                Ok(())
            }
            Self::AcceptedRemoved { required, copied }
                if request.truncation == MqTruncation::Accept
                    && request.mode.removes_message()
                    && required > capacity
                    && copied <= capacity =>
            {
                Ok(())
            }
            Self::AcceptedBrowsed { required, copied }
                if request.truncation == MqTruncation::Accept
                    && !request.mode.removes_message()
                    && required > capacity
                    && copied <= capacity =>
            {
                Ok(())
            }
            _ => Err(MqMessageProblem::Truncation),
        }
    }
}

/// Get result shape. A zero-length body is a message, not a no-message result.
/// Exact MQ completion/reason mapping remains with the later delivery lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqGetDisposition {
    Message(MqTruncationDisposition),
    NoMessage,
    WaitExpired,
    UnknownOutcome,
}

impl MqGetDisposition {
    pub fn validate(self, request: &MqGetContract) -> Result<(), MqMessageProblem> {
        match self {
            Self::Message(truncation) => truncation.validate(request),
            Self::NoMessage if request.wait == MqWait::NoWait => Ok(()),
            Self::WaitExpired if matches!(request.wait, MqWait::BoundedHostTicks(_)) => Ok(()),
            Self::UnknownOutcome => Ok(()),
            _ => Err(MqMessageProblem::Wait),
        }
    }
}

/// Deliberately lacks an exactly-once variant. Numeric completion/reason
/// mapping belongs to later source-bound execution work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqDeliveryOutcome {
    Pending,
    Accepted,
    /// Item failure; exact MQ completion/reason mapping remains pending.
    Rejected,
    DuplicatePossible,
    UnknownOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqDistributionItemResult {
    pub destination: String,
    pub outcome: MqDeliveryOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqDistributionResult {
    pub items: Vec<MqDistributionItemResult>,
}

impl MqDistributionResult {
    pub fn validate(&self, limits: MqMessageLimits) -> Result<(), MqMessageProblem> {
        limits.validate()?;
        if self.items.is_empty() || self.items.len() > limits.distribution_items {
            return Err(MqMessageProblem::Distribution);
        }
        for item in &self.items {
            if validate_name(&item.destination, limits.destination_bytes, false).is_err() {
                return Err(MqMessageProblem::Distribution);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mq_mqi_contract_by_label;

    fn message() -> MqMessage {
        MqMessage {
            descriptor: MqMessageDescriptor {
                identifiers: MqMessageIdentifiers::default(),
                format: Some("MQFMT_STRING".into()),
                expiry: MqExpiry::Unlimited,
                persistence: MqPersistence::QueueDefault,
                priority: MqPriority::QueueDefault,
                ordering: MqMessageOrdering::default(),
            },
            body: Vec::new(),
            properties: Vec::new(),
        }
    }

    fn get() -> MqGetContract {
        MqGetContract {
            selection: MqMessageMatch::default(),
            mode: MqGetMode::Remove,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Reject,
            buffer_capacity: 4,
        }
    }

    #[test]
    fn every_source_identity_joins_the_pinned_catalog() {
        assert_eq!(MQ_MESSAGE_SOURCES.len(), 11);
        for source in MQ_MESSAGE_SOURCES {
            let contract = mq_mqi_contract_by_label(source.call).unwrap();
            assert!(contract.official_row.ends_with(source.row));
            assert_eq!(contract.topic_path, source.topic_path);
            assert_eq!(contract.topic_sha256, source.topic_sha256);
        }
        assert!(MQ_MESSAGE_PENDING.contains(&MqMessagePending::DeliveryAndRecovery));
    }

    #[test]
    fn product_limits_cannot_be_zero_or_widened() {
        let mut limits = MqMessageLimits::default();
        assert_eq!(limits.validate(), Ok(()));
        limits.properties = 0;
        assert_eq!(limits.validate(), Err(MqMessageProblem::Limits));
        limits = MqMessageLimits::default();
        limits.body_bytes += 1;
        assert_eq!(limits.validate(), Err(MqMessageProblem::Limits));
        limits = MqMessageLimits::default();
        limits.wait_ticks += 1;
        assert_eq!(limits.validate(), Err(MqMessageProblem::Limits));
    }

    #[test]
    fn descriptor_and_body_boundaries() {
        let limits = MqMessageLimits::default();
        let mut value = message();
        value.body = vec![0; limits.body_bytes];
        assert_eq!(value.validate(limits), Ok(()));
        value.body.push(0);
        assert_eq!(value.validate(limits), Err(MqMessageProblem::BodyTooLong));
        value.body.clear();
        value.descriptor.identifiers.message_id = Some(vec![1; limits.identifier_bytes]);
        assert_eq!(value.validate(limits), Ok(()));
        value.descriptor.identifiers.message_id = Some(vec![1; limits.identifier_bytes + 1]);
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Identifier));
        value.descriptor.identifiers.message_id = Some(vec![]);
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Identifier));
        value.descriptor.identifiers.message_id = None;
        value.descriptor.format = Some("x".repeat(limits.format_bytes + 1));
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Format));
        value.descriptor.format = None;
        value.descriptor.expiry = MqExpiry::RelativeHostTicks(0);
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Expiry));
    }

    #[test]
    fn property_type_lengths_and_collection_bounds() {
        let limits = MqMessageLimits::default();
        let mut value = message();
        value.properties.push(MqMessageProperty {
            name: "usr.Color".into(),
            kind: MqPropertyType::Int64,
            value: vec![0; 8],
        });
        assert_eq!(value.validate(limits), Ok(()));
        value.properties[0].value.pop();
        assert_eq!(
            value.validate(limits),
            Err(MqMessageProblem::PropertyValueLength)
        );
        value.properties[0].kind = MqPropertyType::Null;
        value.properties[0].value.clear();
        assert_eq!(value.validate(limits), Ok(()));
        value.properties[0].name = "".into();
        assert_eq!(value.validate(limits), Err(MqMessageProblem::PropertyName));
        value.properties[0].name = "usr.Color".into();
        value.properties.push(value.properties[0].clone());
        assert_eq!(
            value.validate(limits),
            Err(MqMessageProblem::DuplicateProperty)
        );
        value.properties = (0..=limits.properties)
            .map(|index| MqMessageProperty {
                name: format!("usr.{index}"),
                kind: MqPropertyType::Null,
                value: Vec::new(),
            })
            .collect();
        assert_eq!(value.validate(limits), Err(MqMessageProblem::PropertyCount));
        value.properties = vec![MqMessageProperty {
            name: "x".into(),
            kind: MqPropertyType::ByteString,
            value: vec![0; limits.property_value_bytes + 1],
        }];
        assert_eq!(
            value.validate(limits),
            Err(MqMessageProblem::PropertyValueLength)
        );
        let narrow = MqMessageLimits {
            property_total_bytes: 2,
            ..limits
        };
        value.properties[0].value = vec![0; 2];
        assert_eq!(
            value.validate(narrow),
            Err(MqMessageProblem::PropertyTotalLength)
        );
    }

    #[test]
    fn every_fixed_property_type_enforces_its_source_length() {
        let limits = MqMessageLimits::default();
        for (kind, width) in [
            (MqPropertyType::Boolean, 4),
            (MqPropertyType::Int8, 1),
            (MqPropertyType::Int16, 2),
            (MqPropertyType::Int32, 4),
            (MqPropertyType::Int64, 8),
            (MqPropertyType::Float32, 4),
            (MqPropertyType::Float64, 8),
            (MqPropertyType::Null, 0),
        ] {
            let mut property = MqMessageProperty {
                name: "usr.Value".into(),
                kind,
                value: vec![0; width],
            };
            assert_eq!(property.validate(limits), Ok(()));
            property.value.push(0);
            assert_eq!(
                property.validate(limits),
                Err(MqMessageProblem::PropertyValueLength)
            );
        }
        for kind in [MqPropertyType::ByteString, MqPropertyType::String] {
            let mut property = MqMessageProperty {
                name: "usr.Value".into(),
                kind,
                value: vec![0; limits.property_value_bytes],
            };
            assert_eq!(property.validate(limits), Ok(()));
            property.value.push(0);
            assert_eq!(
                property.validate(limits),
                Err(MqMessageProblem::PropertyValueLength)
            );
        }
    }

    #[test]
    fn name_and_collection_ceilings_are_inclusive() {
        let limits = MqMessageLimits::default();
        let mut property = MqMessageProperty {
            name: "N".repeat(limits.property_name_bytes),
            kind: MqPropertyType::Null,
            value: Vec::new(),
        };
        assert_eq!(property.validate(limits), Ok(()));
        property.name.push('N');
        assert_eq!(
            property.validate(limits),
            Err(MqMessageProblem::PropertyName)
        );
        let mut report = MqDistributionResult {
            items: (0..limits.distribution_items)
                .map(|index| MqDistributionItemResult {
                    destination: format!("Q{index}"),
                    outcome: MqDeliveryOutcome::Pending,
                })
                .collect(),
        };
        assert_eq!(report.validate(limits), Ok(()));
        report.items[0].destination = "D".repeat(limits.destination_bytes + 1);
        assert_eq!(report.validate(limits), Err(MqMessageProblem::Distribution));
    }

    #[test]
    fn property_query_wildcard_is_terminal_and_inquiry_only() {
        let limits = MqMessageLimits::default();
        assert_eq!(
            MqPropertyQuery::Prefix(String::new()).validate(limits),
            Ok(())
        );
        assert_eq!(
            MqPropertyQuery::Exact("usr.Color".into()).validate(limits),
            Ok(())
        );
        for query in [
            MqPropertyQuery::Exact("".into()),
            MqPropertyQuery::Prefix("usr.%".into()),
        ] {
            assert_eq!(query.validate(limits), Err(MqMessageProblem::PropertyName));
        }
    }

    #[test]
    fn group_and_segment_combinations_are_coherent() {
        let limits = MqMessageLimits::default();
        let mut value = message();
        value.descriptor.ordering.last_in_group = true;
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Group));
        value.descriptor.identifiers.group_id = Some(vec![1]);
        value.descriptor.ordering.group_sequence = Some(0);
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Group));
        value.descriptor.ordering.group_sequence = Some(1);
        assert_eq!(value.validate(limits), Ok(()));
        value.descriptor.ordering.last_segment = true;
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Segment));
        value.descriptor.ordering.segment_offset = Some(limits.body_bytes as u64);
        value.body.push(1);
        assert_eq!(value.validate(limits), Err(MqMessageProblem::Segment));
        value.descriptor.ordering.segment_offset = Some(0);
        assert_eq!(value.validate(limits), Ok(()));
    }

    #[test]
    fn match_browse_wait_and_truncation_dispositions() {
        let limits = MqMessageLimits::default();
        let mut request = get();
        request.buffer_capacity = 0;
        assert_eq!(request.validate(limits), Ok(()));
        request.buffer_capacity = 4;
        request.selection.identifiers.correlation_id = Some(vec![]);
        assert_eq!(request.validate(limits), Err(MqMessageProblem::Identifier));
        request.selection.identifiers.correlation_id = None;
        request.wait = MqWait::BoundedHostTicks(0);
        assert_eq!(request.validate(limits), Err(MqMessageProblem::Wait));
        request.wait = MqWait::BoundedHostTicks(limits.wait_ticks);
        assert_eq!(request.validate(limits), Ok(()));
        request.wait = MqWait::BoundedHostTicks(limits.wait_ticks + 1);
        assert_eq!(request.validate(limits), Err(MqMessageProblem::Wait));
        request.wait = MqWait::NoWait;
        request.mode = MqGetMode::BrowseNext { cursor: 0 };
        assert_eq!(request.validate(limits), Err(MqMessageProblem::Cursor));
        request.mode = MqGetMode::RemoveUnderCursor { cursor: 0 };
        assert_eq!(request.validate(limits), Err(MqMessageProblem::Cursor));
        request.mode = MqGetMode::BrowseNext { cursor: 1 };
        assert_eq!(request.validate(limits), Ok(()));
        request.buffer_capacity = limits.body_bytes + 1;
        assert_eq!(request.validate(limits), Err(MqMessageProblem::BodyTooLong));
        request.buffer_capacity = 4;
        request.mode = MqGetMode::Remove;
        assert_eq!(
            MqTruncationDisposition::RejectedRetained {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Ok(())
        );
        assert_eq!(
            MqTruncationDisposition::AcceptedRemoved {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Err(MqMessageProblem::Truncation)
        );
        request.truncation = MqTruncation::Accept;
        assert_eq!(
            MqTruncationDisposition::AcceptedRemoved {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Ok(())
        );
        request.mode = MqGetMode::BrowseFirst;
        assert_eq!(
            MqTruncationDisposition::AcceptedRemoved {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Err(MqMessageProblem::Truncation)
        );
        assert_eq!(
            MqTruncationDisposition::AcceptedBrowsed {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Ok(())
        );
        request.mode = MqGetMode::RemoveUnderCursor { cursor: 1 };
        assert_eq!(
            MqTruncationDisposition::AcceptedRemoved {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Ok(())
        );
        assert_eq!(
            MqTruncationDisposition::AcceptedBrowsed {
                required: 5,
                copied: 4
            }
            .validate(&request),
            Err(MqMessageProblem::Truncation)
        );
        assert_eq!(
            MqTruncationDisposition::Complete { length: 5 }.validate(&request),
            Err(MqMessageProblem::Truncation)
        );
    }

    #[test]
    fn distribution_results_are_bounded_and_uncertainty_is_explicit() {
        let limits = MqMessageLimits::default();
        let mut result = MqDistributionResult { items: vec![] };
        assert_eq!(result.validate(limits), Err(MqMessageProblem::Distribution));
        result.items.push(MqDistributionItemResult {
            destination: "QUEUE.A".into(),
            outcome: MqDeliveryOutcome::UnknownOutcome,
        });
        assert_eq!(result.validate(limits), Ok(()));
        result.items[0].outcome = MqDeliveryOutcome::DuplicatePossible;
        assert_eq!(result.validate(limits), Ok(()));
        result.items[0].outcome = MqDeliveryOutcome::Rejected;
        assert_eq!(result.validate(limits), Ok(()));
        result.items[0].destination = "".into();
        assert_eq!(result.validate(limits), Err(MqMessageProblem::Distribution));
        result.items = (0..=limits.distribution_items)
            .map(|index| MqDistributionItemResult {
                destination: format!("Q{index}"),
                outcome: MqDeliveryOutcome::Pending,
            })
            .collect();
        assert_eq!(result.validate(limits), Err(MqMessageProblem::Distribution));
    }

    #[test]
    fn get_no_message_wait_and_zero_length_message_stay_distinct() {
        let mut request = get();
        assert_eq!(MqGetDisposition::NoMessage.validate(&request), Ok(()));
        assert_eq!(
            MqGetDisposition::WaitExpired.validate(&request),
            Err(MqMessageProblem::Wait)
        );
        assert_eq!(
            MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 0 })
                .validate(&request),
            Ok(())
        );
        request.wait = MqWait::BoundedHostTicks(1);
        assert_eq!(MqGetDisposition::WaitExpired.validate(&request), Ok(()));
        assert_eq!(
            MqGetDisposition::NoMessage.validate(&request),
            Err(MqMessageProblem::Wait)
        );
        assert_eq!(MqGetDisposition::UnknownOutcome.validate(&request), Ok(()));
    }
}
