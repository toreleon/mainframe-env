//! One message-family transition held under the actual registry's exclusive borrow.
//! No provisional observation is executable, including after an aborted slot is reused.

use super::*;

enum Change {
    Create {
        id: HandleId,
        entry: Entry,
        append: bool,
    },
    Delete {
        id: HandleId,
    },
    Observe,
}

/// Bounded message-slot staging, not host, SAF, UOW or publication permission.
/// The exclusive borrow prevents parent/epoch/slot mutation until adoption or
/// abort. No live entry, count or generation changes during preparation. Capacity
/// is reserved before publication; allocation failure is a known refusal.
/// Dropping this value aborts only staging. It performs no lifetime decision.
pub struct MqMessageCandidate<'a> {
    registry: &'a mut MqHandleRegistry,
    change: Change,
    observation: Option<MqHmsg>,
}

/// Consuming adoption on the exact borrowed registry, after known publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMessageAdoption {
    /// Newly issued executable identity with the provisional canonical identity.
    Created(MqHmsg),
    /// The previously live message entry was retired exactly once.
    Deleted,
    /// Read/property-only use leaves all registry entries unchanged.
    Unchanged,
}

impl MqMessageCandidate<'_> {
    /// Historical/non-executable prospective create observation. The historical
    /// disposition remains permanent even after adoption or later allocation.
    /// Canonical identity excludes that disposition; it never grants execution.
    pub fn provisional(&self) -> Option<MqHmsg> {
        self.observation
    }

    /// Adopt under the same exclusive registry borrow. Storage/SAF/control
    /// verification must have succeeded externally; this method asserts none.
    /// Preparation reserves capacity and checks arithmetic, so no allocation or
    /// new refusal remains here. Only this consuming path may issue a live token.
    pub fn adopt(self) -> MqMessageAdoption {
        match self.change {
            Change::Create {
                mut id,
                entry,
                append,
            } => {
                if append {
                    self.registry.slots.push(Slot {
                        generation: id.generation,
                        entry: None,
                    });
                }
                self.registry.slots[id.slot as usize].entry = Some(entry);
                self.registry.active += 1;
                id.historical = false;
                MqMessageAdoption::Created(MqHmsg(id))
            }
            Change::Delete { id } => {
                self.registry.retire(id);
                MqMessageAdoption::Deleted
            }
            Change::Observe => MqMessageAdoption::Unchanged,
        }
    }

    /// Explicit known abort; all provisional observations stay non-executable.
    pub fn abort(self) {}
}

impl MqHandleRegistry {
    /// Prepare associated message creation without installing a live slot.
    /// This first staging profile requires an issued ordinary nonshared batch
    /// connection. Existing legacy/special/shared constructors remain unchanged.
    pub fn stage_message_create(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
    ) -> Result<MqMessageCandidate<'_>, MqHandleProblem> {
        let parent = self.ordinary_message_parent(owner, connection)?;
        if self.active >= self.max_slots {
            return Err(MqHandleProblem::Capacity);
        }
        self.active
            .checked_add(1)
            .ok_or(MqHandleProblem::Capacity)?;
        let (index, generation, append) = match self
            .slots
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.entry.is_none() && slot.generation != 0)
        {
            Some((index, slot)) => (index, slot.generation, false),
            None => {
                if self.slots.len() >= self.max_slots {
                    return Err(MqHandleProblem::Capacity);
                }
                self.slots
                    .try_reserve(1)
                    .map_err(|_| MqHandleProblem::Capacity)?;
                (self.slots.len(), 1, true)
            }
        };
        let id = HandleId {
            registry: self.registry_id,
            slot: u32::try_from(index).map_err(|_| MqHandleProblem::Capacity)?,
            generation,
            epoch: self.epoch,
            historical: true,
        };
        Ok(MqMessageCandidate {
            registry: self,
            observation: Some(MqHmsg(id)),
            change: Change::Create {
                id,
                entry: Entry {
                    kind: EntryKind::Message,
                    owner,
                    parent: Some(parent),
                    sharing: MqHandleSharing::NonShared,
                    in_use: false,
                },
                append,
            },
        })
    }

    /// Exclusively retain actual message-property lifetime/in-use authority.
    /// Historical/foreign/stale/closed/special/shared identities fail before
    /// staging. This borrow grants no property mutation or publication permit.
    pub fn stage_message_use(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        message: MqHmsg,
    ) -> Result<MqMessageCandidate<'_>, MqHandleProblem> {
        self.ordinary_message_parent(owner, connection)?;
        self.validate_message_property(owner, connection, message.into())?;
        Ok(MqMessageCandidate {
            registry: self,
            change: Change::Observe,
            observation: None,
        })
    }

    /// Stage one retirement; no entry or generation changes until adoption.
    pub fn stage_message_delete(
        &mut self,
        owner: MqHandleOwner,
        connection: MqHconn,
        message: MqHmsg,
    ) -> Result<MqMessageCandidate<'_>, MqHandleProblem> {
        self.ordinary_message_parent(owner, connection)?;
        self.validate_message_property(owner, connection, message.into())?;
        Ok(MqMessageCandidate {
            registry: self,
            change: Change::Delete { id: message.0 },
            observation: None,
        })
    }

    fn ordinary_message_parent(
        &self,
        owner: MqHandleOwner,
        connection: MqHconn,
    ) -> Result<HandleId, MqHandleProblem> {
        if owner.environment != MqHostEnvironment::ZosBatch {
            return Err(MqHandleProblem::SpecialConnection);
        }
        let MqHconn::Issued(_) = connection else {
            return Err(MqHandleProblem::SpecialConnection);
        };
        let parent = self.connection_id(owner, connection)?;
        if self.entry(parent)?.sharing != MqHandleSharing::NonShared {
            return Err(MqHandleProblem::SpecialConnection);
        }
        Ok(parent)
    }
}

#[cfg(test)]
mod tests;
