//! Additive strict @2 DTOs, converted to the sole live checkpoint validator.
use super::super::restart::{LiveMessage, SnapshotProperty};
use super::*;
use mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding;
use mainframe_env_host_api::mq_mqi::{MqFullMessage, mq_md_value_bytes, mq_md_value_decode};

pub(super) const LIVE_SCHEMA: &str = "mainframe-env.mq-delivery-live@2";
const COLD_SCHEMA: &str = "mainframe-env.mq-delivery@2";

pub(super) enum StoredMessage {
    Partial(SnapshotMessage),
    Complete(Box<MqFullMessage>),
}
pub(super) mod legacy_message {
    use super::*;
    pub(in crate::delivery::checkpoint) fn serialize<S: serde::Serializer>(
        v: &StoredMessage,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match v {
            StoredMessage::Partial(m) => LiveMessage::serialize(m, s),
            _ => Err(serde::ser::Error::custom("full payload requires schema @2")),
        }
    }
    pub(in crate::delivery::checkpoint) fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<StoredMessage, D::Error> {
        LiveMessage::deserialize(d).map(StoredMessage::Partial)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Characters {
    AsciiCompatible,
    OwnedCp037,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "kebab-case")]
enum Profile {
    Partial,
    Complete {
        version: i32,
        characters: Characters,
    },
}
impl Profile {
    fn from_runtime(v: QueueProfile) -> Self {
        match v {
            QueueProfile::Partial => Self::Partial,
            QueueProfile::Complete {
                version,
                characters,
            } => Self::Complete {
                version,
                characters: match characters {
                    MqMdCharacterEncoding::AsciiCompatible => Characters::AsciiCompatible,
                    MqMdCharacterEncoding::OwnedCp037 => Characters::OwnedCp037,
                },
            },
        }
    }
    fn into_runtime(self) -> Result<QueueProfile, MqDeliveryError> {
        Ok(match self {
            Self::Partial => QueueProfile::Partial,
            Self::Complete {
                version,
                characters,
            } => {
                if !matches!(version, 1 | 2) {
                    return Err(MqDeliveryError::UnsupportedSchema);
                }
                QueueProfile::Complete {
                    version,
                    characters: match characters {
                        Characters::AsciiCompatible => MqMdCharacterEncoding::AsciiCompatible,
                        Characters::OwnedCp037 => MqMdCharacterEncoding::OwnedCp037,
                    },
                }
            }
        })
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PartialMessage {
    #[serde(with = "LiveMessage")]
    message: SnapshotMessage,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CompleteMessage {
    md: Vec<u8>,
    body: Vec<u8>,
    properties: Vec<SnapshotProperty>,
}
#[derive(Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    deny_unknown_fields,
    rename_all = "kebab-case"
)]
enum Message {
    Partial(PartialMessage),
    Complete(CompleteMessage),
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EntryTwo {
    id: u64,
    #[serde(deserialize_with = "required_option")]
    expires_at: Option<u64>,
    persistent: bool,
    message: Message,
}
impl EntryTwo {
    fn from_live(v: &LiveEntry) -> Result<Self, MqDeliveryError> {
        let message = match &v.message {
            StoredMessage::Partial(m) => Message::Partial(PartialMessage { message: m.clone() }),
            StoredMessage::Complete(m) => Message::Complete(CompleteMessage {
                md: mq_md_value_bytes(&m.descriptor, 2048)
                    .map_err(|_| MqDeliveryError::CorruptSnapshot)?,
                body: m.body.clone(),
                properties: m
                    .properties
                    .iter()
                    .map(SnapshotProperty::from_property)
                    .collect(),
            }),
        };
        Ok(Self {
            id: v.id,
            expires_at: v.expires_at,
            persistent: v.persistent,
            message,
        })
    }
    fn into_live(self) -> Result<LiveEntry, MqDeliveryError> {
        let message = match self.message {
            Message::Partial(m) => StoredMessage::Partial(m.message),
            Message::Complete(m) => StoredMessage::Complete(Box::new(MqFullMessage {
                descriptor: mq_md_value_decode(&m.md, 2048)
                    .map_err(|_| MqDeliveryError::CorruptSnapshot)?,
                body: m.body,
                properties: m
                    .properties
                    .into_iter()
                    .map(SnapshotProperty::into_property)
                    .collect::<Result<_, _>>()?,
            })),
        };
        Ok(LiveEntry {
            id: self.id,
            expires_at: self.expires_at,
            persistent: self.persistent,
            message,
        })
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::delivery::checkpoint) struct QueueTwo {
    name: MqObjectName,
    profile: Profile,
    messages: Vec<EntryTwo>,
}
impl QueueTwo {
    pub(super) fn from_live(v: &LiveQueue) -> Result<Self, MqDeliveryError> {
        Ok(Self {
            name: v.name.clone(),
            profile: Profile::from_runtime(v.profile),
            messages: v
                .messages
                .iter()
                .map(EntryTwo::from_live)
                .collect::<Result<_, _>>()?,
        })
    }
    pub(super) fn into_live(self) -> Result<LiveQueue, MqDeliveryError> {
        Ok(LiveQueue {
            name: self.name,
            profile: self.profile.into_runtime()?,
            messages: self
                .messages
                .into_iter()
                .map(EntryTwo::into_live)
                .collect::<Result<_, _>>()?,
        })
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationTwo {
    queue: MqObjectName,
    put: bool,
    entry: EntryTwo,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::delivery::checkpoint) struct UnitTwo {
    unit: u64,
    operations: Vec<OperationTwo>,
}
impl UnitTwo {
    pub(super) fn from_live(v: &LiveUnit) -> Result<Self, MqDeliveryError> {
        Ok(Self {
            unit: v.unit,
            operations: v
                .operations
                .iter()
                .map(|o| {
                    Ok(OperationTwo {
                        queue: o.queue.clone(),
                        put: o.put,
                        entry: EntryTwo::from_live(&o.entry)?,
                    })
                })
                .collect::<Result<_, MqDeliveryError>>()?,
        })
    }
    pub(super) fn into_live(self) -> Result<LiveUnit, MqDeliveryError> {
        Ok(LiveUnit {
            unit: self.unit,
            operations: self
                .operations
                .into_iter()
                .map(|o| {
                    Ok(LiveOperation {
                        queue: o.queue,
                        put: o.put,
                        entry: o.entry.into_live()?,
                    })
                })
                .collect::<Result<_, MqDeliveryError>>()?,
        })
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Projection {
    schema_version: String,
    manager: MqObjectName,
    default_persistent: bool,
    tick: u64,
    next_id: u64,
    next_cursor: u64,
    queues: Vec<QueueTwo>,
    pending: Vec<UnitTwo>,
    finalized: Vec<LiveFinalized>,
    cursors: Vec<LiveCursor>,
}
impl Projection {
    fn from_live(v: &Checkpoint) -> Result<Self, MqDeliveryError> {
        Ok(Self {
            schema_version: LIVE_SCHEMA.into(),
            manager: v.manager.clone(),
            default_persistent: v.default_persistent,
            tick: v.tick,
            next_id: v.next_id,
            next_cursor: v.next_cursor,
            queues: v
                .queues
                .iter()
                .map(QueueTwo::from_live)
                .collect::<Result<_, _>>()?,
            pending: v
                .pending
                .iter()
                .map(UnitTwo::from_live)
                .collect::<Result<_, _>>()?,
            finalized: v
                .finalized
                .iter()
                .map(|v| LiveFinalized {
                    unit: v.unit,
                    committed: v.committed,
                })
                .collect(),
            cursors: v
                .cursors
                .iter()
                .map(|v| LiveCursor {
                    token: v.token,
                    queue: v.queue.clone(),
                    entry_id: v.entry_id,
                })
                .collect(),
        })
    }
    #[cfg(test)]
    fn into_live(self) -> Result<Checkpoint, MqDeliveryError> {
        Ok(Checkpoint {
            schema_version: LIVE_SCHEMA.into(),
            manager: self.manager,
            default_persistent: self.default_persistent,
            tick: self.tick,
            next_id: self.next_id,
            next_cursor: self.next_cursor,
            queues: self
                .queues
                .into_iter()
                .map(QueueTwo::into_live)
                .collect::<Result<_, _>>()?,
            pending: self
                .pending
                .into_iter()
                .map(UnitTwo::into_live)
                .collect::<Result<_, _>>()?,
            finalized: self.finalized,
            cursors: self.cursors,
        })
    }
}
pub(super) fn encode(v: &Checkpoint, limit: usize) -> Result<Vec<u8>, MqDeliveryError> {
    encode_projection(&Projection::from_live(v)?, limit)
}
pub(in crate::delivery) fn encode_cold(
    kernel: &MqDeliveryKernel,
) -> Result<Vec<u8>, MqDeliveryError> {
    let mut candidate = kernel.clone();
    candidate.purge_expired();
    for (name, entries) in &mut candidate.queues {
        entries.retain(Entry::persistent);
        for ops in kernel.pending.values() {
            for op in ops {
                if let Pending::Get { queue, entry } = op
                    && queue == name
                    && entry.persistent()
                    && entry.expires_at.is_none_or(|e| e > kernel.tick)
                {
                    entries.push(entry.clone());
                }
            }
        }
        entries.sort_by_key(|e| e.id);
    }
    candidate.pending.clear();
    candidate.cursors.clear();
    let mut projection = Projection::from_live(&candidate.live_projection()?)?;
    projection.schema_version = COLD_SCHEMA.into();
    encode_projection(&projection, kernel.limits.snapshot_bytes)
}
impl MqDeliveryKernel {
    /// Strict schema-selected private storage read. Old public readers keep @1.
    #[cfg(test)]
    pub(crate) fn decode_stored(
        bytes: &[u8],
        catalog: &MqObjectCatalog,
        limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        persistence: MqPersistence,
        cold: bool,
    ) -> Result<Self, MqDeliveryError> {
        limits.validate()?;
        message_limits.validate()?;
        preflight::check(bytes, limits, message_limits)?;
        #[derive(Deserialize)]
        struct Header {
            schema_version: String,
        }
        let header: Header =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        if header.schema_version
            == if cold {
                MQ_DELIVERY_SCHEMA
            } else {
                Self::LIVE_CHECKPOINT_SCHEMA
            }
        {
            return if cold {
                Self::decode(bytes, catalog, limits, message_limits, persistence)
            } else {
                Self::decode_live_checkpoint(bytes, catalog, limits, message_limits, persistence)
            };
        }
        if header.schema_version != if cold { COLD_SCHEMA } else { LIVE_SCHEMA } {
            return Err(MqDeliveryError::UnsupportedSchema);
        }
        let projection: Projection =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        if cold
            && (!projection.pending.is_empty()
                || !projection.cursors.is_empty()
                || projection
                    .queues
                    .iter()
                    .flat_map(|q| &q.messages)
                    .any(|m| !m.persistent))
        {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        Self::restore_projection(
            projection.into_live()?,
            Self::new(catalog, limits, message_limits, persistence)?,
        )
    }
}
