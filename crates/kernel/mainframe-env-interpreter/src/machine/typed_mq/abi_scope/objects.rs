//! Object aliases share the root table and never restore registry authority.
use super::*;

pub(in crate::machine::typed_mq) struct ObjectPlan {
    parent: (usize, i32, MqHconn),
    reserved: Option<(usize, i32)>,
    adopted: Option<(usize, i32, MqHobj)>,
    retired: Option<(usize, i32, MqHobj)>,
}
impl MqMqiAbiScope {
    pub(in crate::machine::typed_mq) fn object(
        &self,
        alias: i32,
        connection: MqHconn,
    ) -> Result<MqHobj, HostProblem> {
        let table = self.lock()?;
        if !table
            .slots
            .iter()
            .any(|s| matches!(s, Slot::Connection { token, .. } if *token == connection))
        {
            return Err(HostProblem::Malformed);
        }
        table
            .slots
            .iter()
            .find_map(|s| match s {
                Slot::Object {
                    alias: current,
                    parent,
                    token,
                } if *current == alias && *parent == connection => Some(*token),
                _ => None,
            })
            .ok_or(HostProblem::Malformed)
    }

    pub(in crate::machine::typed_mq) fn object_plan(
        &self,
        connection_alias: i32,
        connection: MqHconn,
        reservation: Option<&Reservation>,
        adopted: Option<MqHobj>,
        retired: Option<i32>,
    ) -> Result<ObjectPlan, HostProblem> {
        if adopted.is_some_and(MqHobj::is_historical) {
            return Err(HostProblem::UnknownOutcome);
        }
        let table = self.lock()?;
        let parent_slot = table
            .slots
            .iter()
            .position(|s| {
                *s == Slot::Connection {
                    alias: connection_alias,
                    token: connection,
                }
            })
            .ok_or(HostProblem::UnknownOutcome)?;
        let reserved = reservation.map(|r| (r.0.slot, r.0.alias));
        if reservation.is_some_and(|r| !std::ptr::eq(self, r.0.scope.as_ref()))
            || reserved
                .is_some_and(|(slot, alias)| table.slots.get(slot) != Some(&Slot::Reserved(alias)))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        // OPEN creates a new object. A duplicate token, even under another
        // parent, cannot be installed as a second executable alias.
        let adopted = adopted
            .map(|token| {
                if table
                    .slots
                    .iter()
                    .any(|s| matches!(s, Slot::Object { token: old, .. } if *old == token))
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                let (slot, alias) = reserved.ok_or(HostProblem::UnknownOutcome)?;
                Ok((slot, alias, token))
            })
            .transpose()?;
        let retired = retired
            .map(|alias| {
                table
                    .slots
                    .iter()
                    .enumerate()
                    .find_map(|(slot, s)| match s {
                        Slot::Object {
                            alias: old,
                            parent,
                            token,
                        } if *old == alias && *parent == connection => Some((slot, alias, *token)),
                        _ => None,
                    })
                    .ok_or(HostProblem::UnknownOutcome)
            })
            .transpose()?;
        Ok(ObjectPlan {
            parent: (parent_slot, connection_alias, connection),
            reserved,
            adopted,
            retired,
        })
    }
}
impl ObjectPlan {
    pub(in crate::machine::typed_mq) fn wire(&self) -> Option<i32> {
        self.adopted.map(|(_, alias, _)| alias)
    }
    pub(in crate::machine::typed_mq) fn guard<'a>(
        &self,
        scope: &'a MqMqiAbiScope,
    ) -> Result<MutexGuard<'a, Table>, HostProblem> {
        let table = scope.lock()?;
        let (slot, alias, connection) = self.parent;
        if table.slots[slot]
            != (Slot::Connection {
                alias,
                token: connection,
            })
            || self
                .reserved
                .is_some_and(|(slot, alias)| table.slots[slot] != Slot::Reserved(alias))
            || self.adopted.is_some_and(|(_, _, token)| {
                table
                    .slots
                    .iter()
                    .any(|s| matches!(s, Slot::Object { token: old, .. } if *old == token))
            })
            || self.retired.is_some_and(|(slot, alias, token)| {
                table.slots[slot]
                    != (Slot::Object {
                        alias,
                        parent: connection,
                        token,
                    })
            })
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(table)
    }
    pub(in crate::machine::typed_mq) fn commit(&self, table: &mut Table) {
        if let Some((slot, _)) = self.reserved {
            table.slots[slot] = Slot::Empty;
        }
        if let Some((slot, alias, token)) = self.adopted {
            table.slots[slot] = Slot::Object {
                alias,
                parent: self.parent.2,
                token,
            };
        }
        if let Some((slot, _, _)) = self.retired {
            table.slots[slot] = Slot::Empty;
        }
    }
}

#[cfg(test)]
mod tests;
