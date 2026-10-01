//! Strict versioned durable pub/sub snapshot codec.

use super::*;
use mainframe_env_host_api::{
    MqExpiry, MqMessageDescriptor, MqMessageIdentifiers, MqMessageOrdering, MqMessageProperty,
    MqPersistence, MqPriority, MqPropertyType,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

impl MqPubsubKernel {
    pub(super) fn validate_state(&self, state: &State) -> Result<(), MqPubsubError> {
        if state.next_sequence == 0
            || state.subscriptions.len() > self.limits.max_subscriptions
            || state.retained.len() > self.limits.max_retained
        {
            return Err(MqPubsubError::ResourceExhausted);
        }
        let mut sequences = BTreeSet::new();
        for (name, item) in &state.subscriptions {
            let (topic, destination, durable) = self.definition(name)?;
            if item.name != *name
                || item.topic != topic
                || item.destination != destination
                || item.durable != durable
                || item.pending.len() > self.limits.max_pending_per_subscription
            {
                return Err(MqPubsubError::CorruptSnapshot);
            }
            let mut prior = 0;
            for delivery in &item.pending {
                delivery.message.validate(self.limits.message)?;
                if delivery.sequence <= prior
                    || delivery.sequence >= state.next_sequence
                    || !sequences.insert(delivery.sequence)
                    || delivery.outcome == MqDeliveryOutcome::Accepted
                {
                    return Err(MqPubsubError::CorruptSnapshot);
                }
                prior = delivery.sequence;
            }
            if let Some(trigger) = &item.trigger
                && (self.trigger_process(&item.destination).is_none()
                    || trigger.sequence == 0
                    || trigger.sequence >= state.next_sequence
                    || item
                        .pending
                        .first()
                        .is_some_and(|delivery| trigger.sequence > delivery.sequence)
                    || trigger.outcome == MqDeliveryOutcome::Accepted)
            {
                return Err(MqPubsubError::CorruptSnapshot);
            }
        }
        for (topic, message) in &state.retained {
            self.describe_publish(topic)?;
            message.validate(self.limits.message)?;
        }
        Ok(())
    }

    fn catalog_digest(&self) -> Result<Vec<u8>, MqPubsubError> {
        Ok(Sha256::digest(self.catalog.encode()?).to_vec())
    }

    pub(super) fn snapshot_state(&self, state: &State) -> Result<Vec<u8>, MqPubsubError> {
        let envelope = Snapshot {
            schema_version: MQ_PUBSUB_SNAPSHOT_SCHEMA.into(),
            catalog_sha256: self.catalog_digest()?,
            next_sequence: state.next_sequence,
            retained: state
                .retained
                .iter()
                .map(|(topic, message)| RetainedRecord {
                    topic: topic.clone(),
                    message: MessageRecord::from(message),
                })
                .collect(),
            subscriptions: state
                .subscriptions
                .values()
                .filter(|item| item.durable)
                .map(SubscriptionRecord::from)
                .collect(),
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|_| MqPubsubError::CorruptSnapshot)?;
        if bytes.len() > self.limits.max_snapshot_bytes {
            return Err(MqPubsubError::ResourceExhausted);
        }
        Ok(bytes)
    }

    /// Snapshot only durable definitions and retained publications. Open UOWs back out on crash.
    pub fn snapshot(&self) -> Result<Vec<u8>, MqPubsubError> {
        self.validate_state(&self.state)?;
        self.snapshot_state(&self.state)
    }

    /// A fresh handle epoch starts stopped. Applications must resume and rebind callbacks.
    pub fn restore(
        bytes: &[u8],
        catalog: MqObjectCatalog,
        limits: MqPubsubLimits,
        epoch: u64,
        max_handles: usize,
    ) -> Result<Self, MqPubsubError> {
        limits.validate()?;
        if bytes.len() > limits.max_snapshot_bytes {
            return Err(MqPubsubError::ResourceExhausted);
        }
        let identity: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| MqPubsubError::CorruptSnapshot)?;
        match identity.get("schema_version") {
            Some(serde_json::Value::String(schema)) if schema == MQ_PUBSUB_SNAPSHOT_SCHEMA => {}
            Some(serde_json::Value::String(_)) => return Err(MqPubsubError::UnsupportedSchema),
            _ => return Err(MqPubsubError::CorruptSnapshot),
        }
        let envelope: Snapshot =
            serde_json::from_slice(bytes).map_err(|_| MqPubsubError::CorruptSnapshot)?;
        let canonical =
            serde_json::to_vec(&envelope).map_err(|_| MqPubsubError::CorruptSnapshot)?;
        if canonical != bytes {
            return Err(MqPubsubError::CorruptSnapshot);
        }
        let mut kernel = Self::new(catalog, limits, epoch, max_handles)?;
        if envelope.catalog_sha256 != kernel.catalog_digest()?
            || envelope.next_sequence == 0
            || envelope.retained.len() > limits.max_retained
            || envelope.subscriptions.len() > limits.max_subscriptions
        {
            return Err(MqPubsubError::CorruptSnapshot);
        }
        let mut state = State {
            subscriptions: BTreeMap::new(),
            retained: BTreeMap::new(),
            next_sequence: envelope.next_sequence,
        };
        let mut previous_topic: Option<MqObjectName> = None;
        for record in envelope.retained {
            if previous_topic
                .as_ref()
                .is_some_and(|name| name >= &record.topic)
            {
                return Err(MqPubsubError::CorruptSnapshot);
            }
            previous_topic = Some(record.topic.clone());
            state
                .retained
                .insert(record.topic, record.message.into_message(limits.message)?);
        }
        let mut previous_name: Option<MqObjectName> = None;
        for record in envelope.subscriptions {
            if previous_name
                .as_ref()
                .is_some_and(|name| name >= &record.name)
                || !record.durable
                || record.pending.len() > limits.max_pending_per_subscription
            {
                return Err(MqPubsubError::CorruptSnapshot);
            }
            previous_name = Some(record.name.clone());
            let subscription = record.into_subscription(limits.message)?;
            state
                .subscriptions
                .insert(subscription.name.clone(), subscription);
        }
        kernel.validate_state(&state)?;
        kernel.state = state;
        Ok(kernel)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: String,
    catalog_sha256: Vec<u8>,
    next_sequence: u64,
    retained: Vec<RetainedRecord>,
    subscriptions: Vec<SubscriptionRecord>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RetainedRecord {
    topic: MqObjectName,
    message: MessageRecord,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionRecord {
    name: MqObjectName,
    topic: MqObjectName,
    destination: MqSubscriptionDestination,
    durable: bool,
    on_request: bool,
    pending: Vec<DeliveryRecord>,
    trigger: Option<TriggerRecord>,
}

impl From<&Subscription> for SubscriptionRecord {
    fn from(value: &Subscription) -> Self {
        Self {
            name: value.name.clone(),
            topic: value.topic.clone(),
            destination: value.destination.clone(),
            durable: value.durable,
            on_request: value.on_request,
            pending: value.pending.iter().map(DeliveryRecord::from).collect(),
            trigger: value.trigger.as_ref().map(TriggerRecord::from),
        }
    }
}

impl SubscriptionRecord {
    fn into_subscription(self, limits: MqMessageLimits) -> Result<Subscription, MqPubsubError> {
        Ok(Subscription {
            name: self.name,
            topic: self.topic,
            destination: self.destination,
            durable: self.durable,
            on_request: self.on_request,
            pending: self
                .pending
                .into_iter()
                .map(|item| item.into_delivery(limits))
                .collect::<Result<_, _>>()?,
            trigger: self.trigger.map(TriggerRecord::into_trigger),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeliveryRecord {
    sequence: u64,
    message: MessageRecord,
    outcome: OutcomeRecord,
}

impl From<&Delivery> for DeliveryRecord {
    fn from(value: &Delivery) -> Self {
        Self {
            sequence: value.sequence,
            message: MessageRecord::from(&value.message),
            outcome: OutcomeRecord::from(&value.outcome),
        }
    }
}

impl DeliveryRecord {
    fn into_delivery(self, limits: MqMessageLimits) -> Result<Delivery, MqPubsubError> {
        Ok(Delivery {
            sequence: self.sequence,
            message: self.message.into_message(limits)?,
            outcome: self.outcome.into_outcome(),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TriggerRecord {
    sequence: u64,
    outcome: OutcomeRecord,
}

impl From<&Trigger> for TriggerRecord {
    fn from(value: &Trigger) -> Self {
        Self {
            sequence: value.sequence,
            outcome: OutcomeRecord::from(&value.outcome),
        }
    }
}

impl TriggerRecord {
    fn into_trigger(self) -> Trigger {
        Trigger {
            sequence: self.sequence,
            outcome: self.outcome.into_outcome(),
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum OutcomeRecord {
    Pending,
    Accepted,
    Rejected,
    DuplicatePossible,
    UnknownOutcome,
}

impl From<&MqDeliveryOutcome> for OutcomeRecord {
    fn from(value: &MqDeliveryOutcome) -> Self {
        match value {
            MqDeliveryOutcome::Pending => Self::Pending,
            MqDeliveryOutcome::Accepted => Self::Accepted,
            MqDeliveryOutcome::Rejected => Self::Rejected,
            MqDeliveryOutcome::DuplicatePossible => Self::DuplicatePossible,
            MqDeliveryOutcome::UnknownOutcome => Self::UnknownOutcome,
        }
    }
}

impl OutcomeRecord {
    fn into_outcome(self) -> MqDeliveryOutcome {
        match self {
            Self::Pending => MqDeliveryOutcome::Pending,
            Self::Accepted => MqDeliveryOutcome::Accepted,
            Self::Rejected => MqDeliveryOutcome::Rejected,
            Self::DuplicatePossible => MqDeliveryOutcome::DuplicatePossible,
            Self::UnknownOutcome => MqDeliveryOutcome::UnknownOutcome,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MessageRecord {
    message_id: Option<Vec<u8>>,
    correlation_id: Option<Vec<u8>>,
    group_id: Option<Vec<u8>>,
    format: Option<String>,
    expiry: ExpiryRecord,
    persistence: PersistenceRecord,
    priority: PriorityRecord,
    group_sequence: Option<u32>,
    last_in_group: bool,
    segment_offset: Option<u64>,
    last_segment: bool,
    segmentation_allowed: bool,
    body: Vec<u8>,
    properties: Vec<PropertyRecord>,
}

impl From<&MqMessage> for MessageRecord {
    fn from(value: &MqMessage) -> Self {
        let descriptor = &value.descriptor;
        Self {
            message_id: descriptor.identifiers.message_id.clone(),
            correlation_id: descriptor.identifiers.correlation_id.clone(),
            group_id: descriptor.identifiers.group_id.clone(),
            format: descriptor.format.clone(),
            expiry: ExpiryRecord::from(descriptor.expiry),
            persistence: PersistenceRecord::from(descriptor.persistence),
            priority: PriorityRecord::from(descriptor.priority),
            group_sequence: descriptor.ordering.group_sequence,
            last_in_group: descriptor.ordering.last_in_group,
            segment_offset: descriptor.ordering.segment_offset,
            last_segment: descriptor.ordering.last_segment,
            segmentation_allowed: descriptor.ordering.segmentation_allowed,
            body: value.body.clone(),
            properties: value.properties.iter().map(PropertyRecord::from).collect(),
        }
    }
}

impl MessageRecord {
    fn into_message(self, limits: MqMessageLimits) -> Result<MqMessage, MqPubsubError> {
        let message = MqMessage {
            descriptor: MqMessageDescriptor {
                identifiers: MqMessageIdentifiers {
                    message_id: self.message_id,
                    correlation_id: self.correlation_id,
                    group_id: self.group_id,
                },
                format: self.format,
                expiry: self.expiry.into_expiry(),
                persistence: self.persistence.into_persistence(),
                priority: self.priority.into_priority(),
                ordering: MqMessageOrdering {
                    group_sequence: self.group_sequence,
                    last_in_group: self.last_in_group,
                    segment_offset: self.segment_offset,
                    last_segment: self.last_segment,
                    segmentation_allowed: self.segmentation_allowed,
                },
            },
            body: self.body,
            properties: self
                .properties
                .into_iter()
                .map(PropertyRecord::into_property)
                .collect::<Result<_, _>>()?,
        };
        message.validate(limits)?;
        Ok(message)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ExpiryRecord {
    Unlimited,
    RelativeHostTicks(u64),
    PendingSource,
}
impl From<MqExpiry> for ExpiryRecord {
    fn from(value: MqExpiry) -> Self {
        match value {
            MqExpiry::Unlimited => Self::Unlimited,
            MqExpiry::RelativeHostTicks(ticks) => Self::RelativeHostTicks(ticks),
            MqExpiry::PendingSource => Self::PendingSource,
        }
    }
}
impl ExpiryRecord {
    fn into_expiry(self) -> MqExpiry {
        match self {
            Self::Unlimited => MqExpiry::Unlimited,
            Self::RelativeHostTicks(ticks) => MqExpiry::RelativeHostTicks(ticks),
            Self::PendingSource => MqExpiry::PendingSource,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PersistenceRecord {
    QueueDefault,
    Persistent,
    NonPersistent,
    PendingSource,
}
impl From<MqPersistence> for PersistenceRecord {
    fn from(value: MqPersistence) -> Self {
        match value {
            MqPersistence::QueueDefault => Self::QueueDefault,
            MqPersistence::Persistent => Self::Persistent,
            MqPersistence::NonPersistent => Self::NonPersistent,
            MqPersistence::PendingSource => Self::PendingSource,
        }
    }
}
impl PersistenceRecord {
    fn into_persistence(self) -> MqPersistence {
        match self {
            Self::QueueDefault => MqPersistence::QueueDefault,
            Self::Persistent => MqPersistence::Persistent,
            Self::NonPersistent => MqPersistence::NonPersistent,
            Self::PendingSource => MqPersistence::PendingSource,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PriorityRecord {
    QueueDefault,
    PendingNumeric(i32),
}
impl From<MqPriority> for PriorityRecord {
    fn from(value: MqPriority) -> Self {
        match value {
            MqPriority::QueueDefault => Self::QueueDefault,
            MqPriority::PendingNumeric(number) => Self::PendingNumeric(number),
        }
    }
}
impl PriorityRecord {
    fn into_priority(self) -> MqPriority {
        match self {
            Self::QueueDefault => MqPriority::QueueDefault,
            Self::PendingNumeric(number) => MqPriority::PendingNumeric(number),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PropertyRecord {
    name: String,
    kind: u8,
    value: Vec<u8>,
}
impl From<&MqMessageProperty> for PropertyRecord {
    fn from(value: &MqMessageProperty) -> Self {
        let kind = match value.kind {
            MqPropertyType::Boolean => 0,
            MqPropertyType::ByteString => 1,
            MqPropertyType::Int8 => 2,
            MqPropertyType::Int16 => 3,
            MqPropertyType::Int32 => 4,
            MqPropertyType::Int64 => 5,
            MqPropertyType::Float32 => 6,
            MqPropertyType::Float64 => 7,
            MqPropertyType::String => 8,
            MqPropertyType::Null => 9,
        };
        Self {
            name: value.name.clone(),
            kind,
            value: value.value.clone(),
        }
    }
}
impl PropertyRecord {
    fn into_property(self) -> Result<MqMessageProperty, MqPubsubError> {
        let kind = match self.kind {
            0 => MqPropertyType::Boolean,
            1 => MqPropertyType::ByteString,
            2 => MqPropertyType::Int8,
            3 => MqPropertyType::Int16,
            4 => MqPropertyType::Int32,
            5 => MqPropertyType::Int64,
            6 => MqPropertyType::Float32,
            7 => MqPropertyType::Float64,
            8 => MqPropertyType::String,
            9 => MqPropertyType::Null,
            _ => return Err(MqPubsubError::CorruptSnapshot),
        };
        Ok(MqMessageProperty {
            name: self.name,
            kind,
            value: self.value,
        })
    }
}
