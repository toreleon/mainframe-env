//! Established cold restart/backout codec; its wire policy is unchanged.

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

impl MqDeliveryKernel {
    /// Restart snapshots contain committed persistent state and backed-out
    /// persistent gets. Staged puts and all cursors are intentionally absent.
    pub fn encode(&self) -> Result<Vec<u8>, MqDeliveryError> {
        self.check_profiles()?;
        if self.schema_two {
            return super::checkpoint::projection::encode_cold(self);
        }
        let mut queues = Vec::with_capacity(self.queues.len());
        for (name, entries) in &self.queues {
            let mut retained: Vec<Entry> = entries
                .iter()
                .filter(|entry| entry.persistent())
                .cloned()
                .collect();
            for operations in self.pending.values() {
                for operation in operations {
                    if let Pending::Get { queue, entry } = operation
                        && queue == name
                        && entry.persistent()
                    {
                        retained.push(entry.clone());
                    }
                }
            }
            retained.sort_by_key(|entry| entry.id);
            queues.push(SnapshotQueue {
                name: name.clone(),
                messages: retained
                    .iter()
                    .map(SnapshotEntry::from_entry)
                    .collect::<Result<_, _>>()?,
            });
        }
        let snapshot = Snapshot {
            schema_version: MQ_DELIVERY_SCHEMA.into(),
            manager: self.manager.clone(),
            tick: self.tick,
            next_id: self.next_id,
            next_cursor: self.next_cursor,
            queues,
            finalized: self
                .finalized
                .iter()
                .map(|(unit, committed)| SnapshotFinalized {
                    unit: *unit,
                    committed: *committed,
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        if bytes.len() > self.limits.snapshot_bytes {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        Ok(bytes)
    }

    pub fn decode(
        bytes: &[u8],
        catalog: &MqObjectCatalog,
        limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        default_persistence: MqPersistence,
    ) -> Result<Self, MqDeliveryError> {
        limits.validate()?;
        if bytes.len() > limits.snapshot_bytes {
            return Err(MqDeliveryError::ResourceExhausted);
        }
        let identity: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        match identity.get("schema_version") {
            Some(serde_json::Value::String(schema)) if schema == MQ_DELIVERY_SCHEMA => {}
            Some(serde_json::Value::String(_)) => return Err(MqDeliveryError::UnsupportedSchema),
            _ => return Err(MqDeliveryError::CorruptSnapshot),
        }
        let snapshot: Snapshot =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        let mut kernel = Self::new(catalog, limits, message_limits, default_persistence)?;
        if snapshot.manager != kernel.manager
            || snapshot.next_id == 0
            || snapshot.next_cursor == 0
            || snapshot.queues.len() != kernel.queues.len()
        {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        kernel.tick = snapshot.tick;
        kernel.next_id = snapshot.next_id;
        kernel.next_cursor = snapshot.next_cursor;
        let mut all_ids = BTreeSet::new();
        let expected_queues: Vec<_> = kernel.queues.keys().cloned().collect();
        for (actual, expected) in snapshot.queues.into_iter().zip(expected_queues) {
            if actual.name != expected {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            let mut prior = 0;
            let mut restored = Vec::with_capacity(actual.messages.len());
            for item in actual.messages {
                if item.id <= prior
                    || item.id >= kernel.next_id
                    || !all_ids.insert(item.id)
                    || item.expires_at.is_some_and(|expiry| expiry <= kernel.tick)
                {
                    return Err(MqDeliveryError::CorruptSnapshot);
                }
                prior = item.id;
                let entry = item.into_entry()?;
                entry
                    .partial()?
                    .validate(message_limits)
                    .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
                validate_supported_message(entry.partial()?)
                    .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
                validate_group_and_segment(&restored, &entry, true)
                    .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
                restored.push(entry);
            }
            kernel
                .queues
                .get_mut(&actual.name)
                .expect("checked queue")
                .entries = restored;
        }
        let mut prior = 0;
        for finalization in snapshot.finalized {
            if finalization.unit <= prior {
                return Err(MqDeliveryError::CorruptSnapshot);
            }
            prior = finalization.unit;
            kernel
                .finalized
                .insert(finalization.unit, finalization.committed);
        }
        kernel
            .check_bounds()
            .map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        Ok(kernel)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: String,
    manager: MqObjectName,
    tick: u64,
    next_id: u64,
    next_cursor: u64,
    queues: Vec<SnapshotQueue>,
    finalized: Vec<SnapshotFinalized>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotQueue {
    name: MqObjectName,
    messages: Vec<SnapshotEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotFinalized {
    unit: u64,
    committed: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnapshotEntry {
    id: u64,
    expires_at: Option<u64>,
    message: SnapshotMessage,
}

impl SnapshotEntry {
    fn from_entry(entry: &Entry) -> Result<Self, MqDeliveryError> {
        Ok(Self {
            id: entry.id,
            expires_at: entry.expires_at,
            message: SnapshotMessage::from_message(entry.partial()?)?,
        })
    }

    fn into_entry(self) -> Result<Entry, MqDeliveryError> {
        Ok(Entry {
            id: self.id,
            expires_at: self.expires_at,
            message: Payload::Partial(self.message.into_message()?),
        })
    }
}

/// Storage projection only. Public requests and results use the frozen host
/// message contract directly.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotMessage {
    message_id: Option<Vec<u8>>,
    correlation_id: Option<Vec<u8>>,
    group_id: Option<Vec<u8>>,
    format: Option<String>,
    expiry_ticks: Option<u64>,
    group_sequence: Option<u32>,
    last_in_group: bool,
    segment_offset: Option<u64>,
    last_segment: bool,
    segmentation_allowed: bool,
    body: Vec<u8>,
    properties: Vec<SnapshotProperty>,
}

// Same message projection, but the live schema requires every field, including
// explicitly null optional fields. The cold reader's historical defaults stay
// unchanged. Remote derive checks duplicate/unknown fields without a Value map
// that would collapse duplicate JSON keys before validation.
#[derive(Deserialize, Serialize)]
#[serde(remote = "SnapshotMessage", deny_unknown_fields)]
pub(super) struct LiveMessage {
    #[serde(deserialize_with = "required_option")]
    message_id: Option<Vec<u8>>,
    #[serde(deserialize_with = "required_option")]
    correlation_id: Option<Vec<u8>>,
    #[serde(deserialize_with = "required_option")]
    group_id: Option<Vec<u8>>,
    #[serde(deserialize_with = "required_option")]
    format: Option<String>,
    #[serde(deserialize_with = "required_option")]
    expiry_ticks: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    group_sequence: Option<u32>,
    last_in_group: bool,
    #[serde(deserialize_with = "required_option")]
    segment_offset: Option<u64>,
    last_segment: bool,
    segmentation_allowed: bool,
    body: Vec<u8>,
    properties: Vec<SnapshotProperty>,
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

impl SnapshotMessage {
    fn from_message(message: &MqMessage) -> Result<Self, MqDeliveryError> {
        if message.descriptor.persistence != MqPersistence::Persistent {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        Self::from_live_message(message)
    }

    pub(super) fn from_live_message(message: &MqMessage) -> Result<Self, MqDeliveryError> {
        if !matches!(
            message.descriptor.persistence,
            MqPersistence::Persistent | MqPersistence::NonPersistent
        ) || message.descriptor.priority != MqPriority::QueueDefault
        {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        let descriptor = &message.descriptor;
        let ids = &descriptor.identifiers;
        let ordering = &descriptor.ordering;
        Ok(Self {
            message_id: ids.message_id.clone(),
            correlation_id: ids.correlation_id.clone(),
            group_id: ids.group_id.clone(),
            format: descriptor.format.clone(),
            expiry_ticks: match descriptor.expiry {
                MqExpiry::Unlimited => None,
                MqExpiry::RelativeHostTicks(value) => Some(value),
                MqExpiry::PendingSource => return Err(MqDeliveryError::CorruptSnapshot),
            },
            group_sequence: ordering.group_sequence,
            last_in_group: ordering.last_in_group,
            segment_offset: ordering.segment_offset,
            last_segment: ordering.last_segment,
            segmentation_allowed: ordering.segmentation_allowed,
            body: message.body.clone(),
            properties: message
                .properties
                .iter()
                .map(SnapshotProperty::from_property)
                .collect(),
        })
    }

    pub(super) fn into_message(self) -> Result<MqMessage, MqDeliveryError> {
        let properties = self
            .properties
            .into_iter()
            .map(SnapshotProperty::into_property)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MqMessage {
            descriptor: MqMessageDescriptor {
                identifiers: MqMessageIdentifiers {
                    message_id: self.message_id,
                    correlation_id: self.correlation_id,
                    group_id: self.group_id,
                },
                format: self.format,
                expiry: self
                    .expiry_ticks
                    .map_or(MqExpiry::Unlimited, MqExpiry::RelativeHostTicks),
                persistence: MqPersistence::Persistent,
                priority: MqPriority::QueueDefault,
                ordering: MqMessageOrdering {
                    group_sequence: self.group_sequence,
                    last_in_group: self.last_in_group,
                    segment_offset: self.segment_offset,
                    last_segment: self.last_segment,
                    segmentation_allowed: self.segmentation_allowed,
                },
            },
            body: self.body,
            properties,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SnapshotProperty {
    name: String,
    kind: String,
    value: Vec<u8>,
}

impl SnapshotProperty {
    pub(super) fn from_property(property: &MqMessageProperty) -> Self {
        let kind = match property.kind {
            MqPropertyType::Boolean => "boolean",
            MqPropertyType::ByteString => "byte-string",
            MqPropertyType::Int8 => "int8",
            MqPropertyType::Int16 => "int16",
            MqPropertyType::Int32 => "int32",
            MqPropertyType::Int64 => "int64",
            MqPropertyType::Float32 => "float32",
            MqPropertyType::Float64 => "float64",
            MqPropertyType::String => "string",
            MqPropertyType::Null => "null",
        };
        Self {
            name: property.name.clone(),
            kind: kind.into(),
            value: property.value.clone(),
        }
    }

    pub(super) fn into_property(self) -> Result<MqMessageProperty, MqDeliveryError> {
        let kind = match self.kind.as_str() {
            "boolean" => MqPropertyType::Boolean,
            "byte-string" => MqPropertyType::ByteString,
            "int8" => MqPropertyType::Int8,
            "int16" => MqPropertyType::Int16,
            "int32" => MqPropertyType::Int32,
            "int64" => MqPropertyType::Int64,
            "float32" => MqPropertyType::Float32,
            "float64" => MqPropertyType::Float64,
            "string" => MqPropertyType::String,
            "null" => MqPropertyType::Null,
            _ => return Err(MqDeliveryError::CorruptSnapshot),
        };
        Ok(MqMessageProperty {
            name: self.name,
            kind,
            value: self.value,
        })
    }
}
