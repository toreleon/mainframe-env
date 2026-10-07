//! Exact storage-only facade over the existing strict message/property projection.
//! Historical cold/live bytes and their persistence/default policies are unchanged.

use super::restart::{SnapshotMessage, SnapshotProperty};
use super::*;
use serde::{Deserialize, Serialize};

macro_rules! simple {
    ($local:ident, $host:ident { $($variant:ident),+ }) => {
        #[derive(Serialize)]
        enum $local { $($variant),+ }
        impl<'de> Deserialize<'de> for $local {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let value = String::deserialize(d)?;
                match value.as_str() {
                    $(stringify!($variant) => Ok(Self::$variant),)+
                    _ => Err(serde::de::Error::custom("unknown stored persistence")),
                }
            }
        }
        impl From<$host> for $local {
            fn from(value: $host) -> Self {
                match value { $($host::$variant => Self::$variant),+ }
            }
        }
        impl From<$local> for $host {
            fn from(value: $local) -> Self {
                match value { $($local::$variant => Self::$variant),+ }
            }
        }
    }
}
simple!(
    Persistence,
    MqPersistence {
        QueueDefault,
        Persistent,
        NonPersistent,
        PendingSource
    }
);

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Expiry {
    Unlimited {},
    RelativeHostTicks { ticks: u64 },
    PendingSource {},
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Priority {
    QueueDefault {},
    PendingNumeric { value: i32 },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayMessage {
    #[serde(with = "super::restart::LiveMessage")]
    projection: SnapshotMessage,
    persistence: Persistence,
    expiry: Expiry,
    priority: Priority,
}
impl ReplayMessage {
    pub(crate) fn from_message(value: &MqMessage) -> Result<Self, MqDeliveryError> {
        // The old projection requires resolved policy. Normalize ONLY its three
        // policy fields, retain them explicitly, and never resolve pending data.
        let mut projection = value.clone();
        projection.descriptor.persistence = MqPersistence::Persistent;
        projection.descriptor.expiry = MqExpiry::Unlimited;
        projection.descriptor.priority = MqPriority::QueueDefault;
        Ok(Self {
            projection: SnapshotMessage::from_live_message(&projection)?,
            persistence: value.descriptor.persistence.into(),
            expiry: match value.descriptor.expiry {
                MqExpiry::Unlimited => Expiry::Unlimited {},
                MqExpiry::RelativeHostTicks(ticks) => Expiry::RelativeHostTicks { ticks },
                MqExpiry::PendingSource => Expiry::PendingSource {},
            },
            priority: match value.descriptor.priority {
                MqPriority::QueueDefault => Priority::QueueDefault {},
                MqPriority::PendingNumeric(value) => Priority::PendingNumeric { value },
            },
        })
    }
    pub(crate) fn into_message(self) -> Result<MqMessage, MqDeliveryError> {
        let mut value = self.projection.into_message()?;
        // There is one exact representation: projection policy must be neutral.
        if value.descriptor.expiry != MqExpiry::Unlimited {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        value.descriptor.persistence = self.persistence.into();
        value.descriptor.expiry = match self.expiry {
            Expiry::Unlimited {} => MqExpiry::Unlimited,
            Expiry::RelativeHostTicks { ticks } => MqExpiry::RelativeHostTicks(ticks),
            Expiry::PendingSource {} => MqExpiry::PendingSource,
        };
        value.descriptor.priority = match self.priority {
            Priority::QueueDefault {} => MqPriority::QueueDefault,
            Priority::PendingNumeric { value } => MqPriority::PendingNumeric(value),
        };
        Ok(value)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct ReplayDescriptor(ReplayMessage);
impl ReplayDescriptor {
    pub(crate) fn from_descriptor(value: &MqMessageDescriptor) -> Result<Self, MqDeliveryError> {
        ReplayMessage::from_message(&MqMessage {
            descriptor: value.clone(),
            body: Vec::new(),
            properties: Vec::new(),
        })
        .map(Self)
    }
    pub(crate) fn into_descriptor(self) -> Result<MqMessageDescriptor, MqDeliveryError> {
        let value = self.0.into_message()?;
        if !value.body.is_empty() || !value.properties.is_empty() {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        Ok(value.descriptor)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct ReplayProperty(SnapshotProperty);
impl ReplayProperty {
    pub(crate) fn from_property(value: &MqMessageProperty) -> Self {
        Self(SnapshotProperty::from_property(value))
    }
    pub(crate) fn into_property(self) -> Result<MqMessageProperty, MqDeliveryError> {
        self.0.into_property()
    }
}
