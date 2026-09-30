//! Bounded MQ object names, definitions, capabilities, and lifecycle DTOs.

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

/// Stable provider-owned snapshot schema for the MQ object kernel.
pub const MQ_OBJECT_CATALOG_SCHEMA: &str = "mainframe-env.mq-object-catalog@1";
/// MQOD and MQCONN use 48-byte object and queue-manager names.
pub const MQ_OBJECT_NAME_BYTES: usize = 48;
const DYNAMIC_SUFFIX_BYTES: usize = 16;

/// Bounds applied before building, resolving, or decoding an object catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqObjectLimits {
    pub max_objects: usize,
    pub max_dynamic_instances: usize,
    pub max_resolution_depth: usize,
    pub max_persisted_bytes: usize,
}

impl Default for MqObjectLimits {
    fn default() -> Self {
        Self {
            max_objects: 4_096,
            max_dynamic_instances: 4_096,
            max_resolution_depth: 32,
            max_persisted_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Closed failure vocabulary for object validation and lifecycle operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqObjectError {
    InvalidName,
    InvalidOwner,
    InvalidLimits,
    DuplicateObject,
    MissingReference,
    InvalidReferenceKind,
    UnknownObject,
    UnsupportedCapability,
    ResolutionCycle,
    ResolutionDepthExceeded,
    ResourceExhausted,
    NameInUse,
    InvalidDynamicPattern,
    InvalidCloseMode,
    ObjectInUse,
    NotAuthorized,
    CorruptSnapshot,
    UnsupportedSchema,
}

impl fmt::Display for MqObjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidName => "invalid MQ object name",
            Self::InvalidOwner => "invalid MQ lifecycle owner",
            Self::InvalidLimits => "invalid MQ object limits",
            Self::DuplicateObject => "duplicate MQ object",
            Self::MissingReference => "missing MQ object reference",
            Self::InvalidReferenceKind => "MQ object reference has an invalid kind",
            Self::UnknownObject => "unknown MQ object",
            Self::UnsupportedCapability => "capability is not valid for the MQ object",
            Self::ResolutionCycle => "MQ object resolution cycle",
            Self::ResolutionDepthExceeded => "MQ object resolution depth exceeded",
            Self::ResourceExhausted => "MQ object resource bound exceeded",
            Self::NameInUse => "MQ queue name is already in use",
            Self::InvalidDynamicPattern => "invalid dynamic queue name pattern",
            Self::InvalidCloseMode => "close mode is invalid for the dynamic queue",
            Self::ObjectInUse => "dynamic queue is not deletable in its current state",
            Self::NotAuthorized => "dynamic queue deletion requires authorization",
            Self::CorruptSnapshot => "corrupt MQ object snapshot",
            Self::UnsupportedSchema => "unsupported MQ object snapshot schema",
        })
    }
}

impl std::error::Error for MqObjectError {}

/// Canonical, case-sensitive MQ object name with permitted trailing blanks removed.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MqObjectName(String);

impl MqObjectName {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectError> {
        let value = value.as_ref();
        if value.len() > MQ_OBJECT_NAME_BYTES {
            return Err(MqObjectError::InvalidName);
        }
        let value = value
            .split_once('\0')
            .map_or(value, |(significant, _)| significant);
        let value = value.trim_end_matches(' ');
        if value.is_empty()
            || value.len() > MQ_OBJECT_NAME_BYTES
            || !value.bytes().all(valid_name_byte)
        {
            return Err(MqObjectError::InvalidName);
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_name_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b'.' | b'/' | b'_' | b'%')
}

impl fmt::Display for MqObjectName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for MqObjectName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for MqObjectName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let canonical = Self::new(&value).map_err(de::Error::custom)?;
        if canonical.as_str() != value {
            return Err(de::Error::custom("MQ object name is not canonical"));
        }
        Ok(canonical)
    }
}

/// Bounded owner identity retained with a dynamic model instance.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MqLifecycleOwner(String);

impl MqLifecycleOwner {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectError> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > 128
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(MqObjectError::InvalidOwner);
        }
        Ok(Self(value.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for MqLifecycleOwner {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for MqLifecycleOwner {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Provider-owned dynamic-name pattern. A wildcard is allowed only as the final byte.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqDynamicQueuePattern(pub(super) String);

impl MqDynamicQueuePattern {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MqObjectError> {
        let value = value.as_ref().trim_end_matches(' ');
        if !value.ends_with('*') {
            return MqObjectName::new(value)
                .map(|name| Self(name.0))
                .map_err(|_| MqObjectError::InvalidDynamicPattern);
        }
        let prefix = value.strip_suffix('*').unwrap_or_default();
        if prefix.is_empty()
            || prefix.len() + DYNAMIC_SUFFIX_BYTES > MQ_OBJECT_NAME_BYTES
            || prefix.bytes().any(|byte| !valid_name_byte(byte))
            || prefix.contains('*')
        {
            return Err(MqObjectError::InvalidDynamicPattern);
        }
        Ok(Self(format!("{prefix}*")))
    }

    pub(super) fn wildcard_prefix(&self) -> Option<&str> {
        self.0.strip_suffix('*')
    }
}

/// Exact object kinds owned by the MQ-1502 kernel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MqObjectKind {
    QueueManager,
    LocalQueue,
    AliasQueue,
    RemoteQueue,
    ModelQueue,
    Topic,
    Subscription,
    Process,
}

/// Object-level capability identity derived from the pinned open/subscription surface.
///
/// `Inquire` records that MQOPEN admits the object for inquiry. It does not implement or
/// claim MQINQ behavior while the pinned MQINQ body is unavailable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MqObjectCapability {
    Input,
    Browse,
    Output,
    Inquire,
    Set,
    Publish,
    Subscribe,
    RequestPublications,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MqLocalQueueUsage {
    Normal,
    Transmission,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MqDynamicQueueKind {
    Temporary,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "target_kind", content = "name", rename_all = "kebab-case")]
pub enum MqAliasTarget {
    Queue(MqObjectName),
    Topic(MqObjectName),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "destination_kind", content = "queue", rename_all = "kebab-case")]
pub enum MqSubscriptionDestination {
    Managed,
    Queue(MqObjectName),
}

/// Versioned application-neutral MQ object definition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MqObjectDefinition {
    LocalQueue {
        name: MqObjectName,
        usage: MqLocalQueueUsage,
        trigger_process: Option<MqObjectName>,
    },
    AliasQueue {
        name: MqObjectName,
        target: MqAliasTarget,
    },
    RemoteQueue {
        name: MqObjectName,
        /// `None` identifies the remote-queue definition as a queue-manager alias.
        remote_queue: Option<MqObjectName>,
        remote_queue_manager: MqObjectName,
        transmission_queue: Option<MqObjectName>,
    },
    ModelQueue {
        name: MqObjectName,
        definition_type: MqDynamicQueueKind,
        trigger_process: Option<MqObjectName>,
    },
    Topic {
        name: MqObjectName,
    },
    Subscription {
        name: MqObjectName,
        topic: MqObjectName,
        destination: MqSubscriptionDestination,
        durable: bool,
    },
    Process {
        name: MqObjectName,
    },
}

impl MqObjectDefinition {
    #[must_use]
    pub fn name(&self) -> &MqObjectName {
        match self {
            Self::LocalQueue { name, .. }
            | Self::AliasQueue { name, .. }
            | Self::RemoteQueue { name, .. }
            | Self::ModelQueue { name, .. }
            | Self::Topic { name }
            | Self::Subscription { name, .. }
            | Self::Process { name } => name,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> MqObjectKind {
        match self {
            Self::LocalQueue { .. } => MqObjectKind::LocalQueue,
            Self::AliasQueue { .. } => MqObjectKind::AliasQueue,
            Self::RemoteQueue { .. } => MqObjectKind::RemoteQueue,
            Self::ModelQueue { .. } => MqObjectKind::ModelQueue,
            Self::Topic { .. } => MqObjectKind::Topic,
            Self::Subscription { .. } => MqObjectKind::Subscription,
            Self::Process { .. } => MqObjectKind::Process,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MqQueueManagerDefinition {
    pub name: MqObjectName,
    pub default_transmission_queue: Option<MqObjectName>,
}

/// Namespace-qualified lookup, avoiding false collisions between queue, topic, process,
/// and subscription names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqObjectLookup {
    QueueManager,
    Queue(MqObjectName),
    Topic(MqObjectName),
    Subscription(MqObjectName),
    Process(MqObjectName),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqObjectIdentity {
    pub kind: MqObjectKind,
    pub name: MqObjectName,
}

/// Routing material passed to a separately selected channel adapter.
///
/// This structure does not implement or emulate an IBM channel wire protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqChannelRoute {
    pub local_definition: MqObjectName,
    pub remote_queue: MqObjectName,
    pub remote_queue_manager: MqObjectName,
    pub transmission_queue: MqObjectName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqResolvedTarget {
    Queue {
        name: MqObjectName,
        dynamic: bool,
        model: Option<MqObjectName>,
    },
    Model {
        name: MqObjectName,
        definition_type: MqDynamicQueueKind,
    },
    Topic {
        name: MqObjectName,
    },
    Remote(MqChannelRoute),
    Definition {
        identity: MqObjectIdentity,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqResolution {
    pub target: MqResolvedTarget,
    pub path: Vec<MqObjectIdentity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MqModelInstance {
    pub name: MqObjectName,
    pub model: MqObjectName,
    pub definition_type: MqDynamicQueueKind,
    pub trigger_process: Option<MqObjectName>,
    pub creator: MqLifecycleOwner,
    pub instance_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqCloseMode {
    Retain,
    Delete,
    DeletePurge,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MqDynamicQueueState {
    pub messages: usize,
    pub pending_updates: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqCloseOutcome {
    Retained,
    Deleted { purged_messages: usize },
}
