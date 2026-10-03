//! Reference-free identity observation. Only historical tokens can be decoded.
//! This is not IBM wire, receipt attestation, SAF or a cold-recovery authority.

use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
enum Role {
    Connection,
    Object,
    Subscription,
    Message,
}
impl<'de> Deserialize<'de> for Role {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Symbol;
        impl serde::de::Visitor<'_> for Symbol {
            type Value = Role;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a fixed observed handle role")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Role, E> {
                match value {
                    "Connection" => Ok(Role::Connection),
                    "Object" => Ok(Role::Object),
                    "Subscription" => Ok(Role::Subscription),
                    "Message" => Ok(Role::Message),
                    _ => Err(E::custom("unknown observed role")),
                }
            }
        }
        d.deserialize_str(Symbol)
    }
}

/// Immutable reference-free identity with a strict fixed-field storage reader.
/// Capturing or parsing this value never validates a live handle or an actor.
/// Its typed factories create ONLY historical, non-executable values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawObservation")]
pub struct MqHandleObservation {
    role: Role,
    registry: u64,
    slot: u32,
    generation: u64,
    epoch: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObservation {
    role: Role,
    registry: u64,
    slot: u32,
    generation: u64,
    epoch: u64,
}
impl TryFrom<RawObservation> for MqHandleObservation {
    type Error = &'static str;
    fn try_from(raw: RawObservation) -> Result<Self, Self::Error> {
        if raw.registry == 0
            || raw.generation == 0
            || raw.epoch == 0
            || raw.slot as usize >= MQ_MAX_HANDLE_SLOTS
        {
            return Err("invalid observed identity");
        }
        Ok(Self {
            role: raw.role,
            registry: raw.registry,
            slot: raw.slot,
            generation: raw.generation,
            epoch: raw.epoch,
        })
    }
}
impl MqHandleObservation {
    fn capture(role: Role, id: HandleId) -> Self {
        Self {
            role,
            registry: id.registry,
            slot: id.slot,
            generation: id.generation,
            epoch: id.epoch,
        }
    }
    fn id(self, historical: bool) -> HandleId {
        HandleId {
            registry: self.registry,
            slot: self.slot,
            generation: self.generation,
            epoch: self.epoch,
            historical,
        }
    }
    fn require_role(self, role: Role) -> Result<(), MqHandleProblem> {
        if self.role != role {
            return Err(MqHandleProblem::WrongKind);
        }
        Ok(())
    }
    /// Read-only identity capture. Symbolic special values cannot retain a
    /// no-authority disposition in the existing enum and are refused.
    pub fn capture_connection(value: MqHconn) -> Result<Self, MqHandleProblem> {
        match value {
            MqHconn::Issued(value) => Ok(Self::capture(Role::Connection, value.0)),
            MqHconn::Default | MqHconn::Unassociated => Err(MqHandleProblem::SpecialConnection),
        }
    }
    /// Construct ONLY a historical issued connection. No registry is accessed.
    pub fn historical_connection(self) -> Result<MqHconn, MqHandleProblem> {
        self.require_role(Role::Connection)?;
        Ok(MqHconn::Issued(MqConnectionId(self.id(true))))
    }
    pub fn historical_object(self) -> Result<MqHobj, MqHandleProblem> {
        self.require_role(Role::Object)?;
        Ok(MqHobj(self.id(true)))
    }
    pub fn historical_subscription(self) -> Result<MqHsub, MqHandleProblem> {
        self.require_role(Role::Subscription)?;
        Ok(MqHsub(self.id(true)))
    }
    pub fn historical_message(self) -> Result<MqHmsg, MqHandleProblem> {
        self.require_role(Role::Message)?;
        Ok(MqHmsg(self.id(true)))
    }
}
impl From<MqHandle> for MqHandleObservation {
    fn from(value: MqHandle) -> Self {
        Self::capture(
            match value.kind() {
                MqHandleKind::Object => Role::Object,
                MqHandleKind::Subscription => Role::Subscription,
                MqHandleKind::Message => Role::Message,
            },
            value.id(),
        )
    }
}
impl MqHconn {
    /// Symbolic specials are never created by historical observation decoding.
    pub const fn is_historical(self) -> bool {
        matches!(self, Self::Issued(value) if value.is_historical())
    }
}
impl MqHandle {
    pub fn is_historical(self) -> bool {
        self.id().historical
    }
}
impl MqHandleRegistry {
    fn observed_entry(
        &self,
        owner: MqHandleOwner,
        observation: MqHandleObservation,
    ) -> Result<HandleId, MqHandleProblem> {
        Self::check_owner(owner)?;
        // This is lookup of an EXISTING entry, not disposition removal on a
        // token. Equality of counters alone cannot pass owner/role/access checks.
        let id = observation.id(false);
        let entry = self.entry(id)?;
        if entry.owner != owner {
            return Err(MqHandleProblem::CrossOwner);
        }
        if entry.in_use {
            return Err(MqHandleProblem::InUse);
        }
        Ok(id)
    }
    /// Resolve ONLY an already-existing issued connection under an exact owner.
    /// Caller must first attest retained receipt/core occurrence/actor and the
    /// current frame. Observation/digest alone attests none of these. No
    /// allocation, rebinding, restart resurrection or state change occurs.
    pub fn resolve_observed_connection(
        &self,
        owner: MqHandleOwner,
        observation: MqHandleObservation,
    ) -> Result<MqHconn, MqHandleProblem> {
        observation.require_role(Role::Connection)?;
        let id = self.observed_entry(owner, observation)?;
        let value = MqHconn::Issued(MqConnectionId(id));
        self.validate_connection(owner, value)?;
        // An observed CICS default's underlying entry cannot be relabelled as
        // an issued connection, even if a forged observation matches its ID.
        if self.defaults.iter().any(|(_, default)| *default == id) {
            return Err(MqHandleProblem::SpecialConnection);
        }
        Ok(value)
    }
    /// Resolve ONLY an existing exact-owner/role/epoch child on an independently
    /// admitted LIVE connection. Service receipt/core/frame/SAF proof remains
    /// a caller obligation; this read-only lookup is not an operation permit.
    pub fn resolve_observed_handle(
        &self,
        owner: MqHandleOwner,
        hconn: MqHconn,
        observation: MqHandleObservation,
        expected: MqHandleKind,
    ) -> Result<MqHandle, MqHandleProblem> {
        let role = match expected {
            MqHandleKind::Object => Role::Object,
            MqHandleKind::Subscription => Role::Subscription,
            MqHandleKind::Message => Role::Message,
        };
        observation.require_role(role)?;
        let id = self.observed_entry(owner, observation)?;
        let value = match expected {
            MqHandleKind::Object => MqHandle::Object(MqHobj(id)),
            MqHandleKind::Subscription => MqHandle::Subscription(MqHsub(id)),
            MqHandleKind::Message => MqHandle::Message(MqHmsg(id)),
        };
        if expected == MqHandleKind::Message {
            self.validate_message_property(owner, hconn, value)?;
        } else {
            self.validate(owner, hconn, value, expected)?;
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
