//! Strict retained provenance rows; no opaque token codec or open fallback.

use super::ownership::*;
use crate::service::{MqLimits, OBJECT_ROW_SCHEMA, ObjectRow, encode_object_row};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub(in crate::service) struct OwnershipRows {
    pub(in crate::service) control: Option<Control>,
    pub(super) units: BTreeMap<u64, UnitOwner>,
    records: BTreeMap<(String, String), ProviderStateRecord>,
}

pub(in crate::service) fn is_ownership_namespace(namespace: &str) -> bool {
    matches!(namespace, CONTROL_NAMESPACE | UOW_NAMESPACE)
}

impl OwnershipRows {
    pub(super) fn require_physical_control(
        &self,
        record: &ProviderStateRecord,
    ) -> Result<(), HostProblem> {
        // A different incarnation/version cannot lend this runtime authority,
        // even when its original reply still has a matching local live entry.
        if self
            .records
            .get(&(CONTROL_NAMESPACE.into(), CONTROL_KEY.into()))
            != Some(record)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }

    pub(in crate::service) fn require_transition_from(
        &self,
        old: &Self,
        limits: MqLimits,
    ) -> Result<(), HostProblem> {
        let control = self.control.as_ref().ok_or(HostProblem::Malformed)?;
        old.changes(control, &self.units, limits)?;
        Ok(())
    }
    pub(in crate::service) fn validate_delivery(
        &self,
        delivery: &crate::MqDeliveryKernel,
        rows: &crate::delivery::checkpoint::rows::DeliveryRows,
    ) -> Result<(), HostProblem> {
        use mainframe_env_host_api::MqDeliveryOutcome;
        if self.control.is_none() {
            // Before selected activation these are the original reader's rows;
            // no new ownership is inferred from legacy/import numeric units.
            return Ok(());
        }
        for record in rows.captured_records().filter(|r| {
            matches!(
                r.namespace.as_str(),
                "mq-delivery-live-v1-pending" | "mq-delivery-live-v1-final"
            )
        }) {
            let id = record
                .key
                .parse::<u64>()
                .map_err(|_| HostProblem::Malformed)?;
            if !self.units.contains_key(&id) {
                return Err(HostProblem::Malformed);
            }
        }
        for unit in self.units.values() {
            for queue in &unit.queues {
                let name = crate::MqObjectName::new(queue).map_err(|_| HostProblem::Malformed)?;
                if delivery.depth(&name).is_none() {
                    return Err(HostProblem::Malformed);
                }
            }
            let valid = match (unit.state, delivery.unit_outcome(unit.unit)) {
                (UnitState::Pending, MqDeliveryOutcome::Pending)
                | (UnitState::Committed, MqDeliveryOutcome::DuplicatePossible)
                | (UnitState::RolledBack, MqDeliveryOutcome::Rejected) => true,
                (_, MqDeliveryOutcome::UnknownOutcome) => unit.queues.is_empty(),
                _ => false,
            };
            if !valid {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }
    pub(in crate::service) fn restore(
        records: &[ProviderStateRecord],
        generation: u64,
        fence: u64,
        limits: MqLimits,
    ) -> Result<Self, HostProblem> {
        let mut value = Self::default();
        let mut bytes = 0usize;
        for record in records
            .iter()
            .filter(|r| is_ownership_namespace(&r.namespace))
        {
            bytes = bytes
                .checked_add(record.payload.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            if bytes > limits.max_state_bytes
                || record.payload.len() > limits.max_state_bytes
                || record.validate_write(limits.max_state_bytes).is_err()
                || value
                    .records
                    .insert(
                        (record.namespace.clone(), record.key.clone()),
                        record.clone(),
                    )
                    .is_some()
            {
                return Err(HostProblem::Malformed);
            }
            match record.namespace.as_str() {
                CONTROL_NAMESPACE => {
                    if record.key != CONTROL_KEY || value.control.is_some() {
                        return Err(HostProblem::Malformed);
                    }
                    let row: ObjectRow<Control> = serde_json::from_slice(&record.payload)
                        .map_err(|_| HostProblem::Malformed)?;
                    if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != record.key {
                        return Err(HostProblem::Malformed);
                    }
                    row.value.validate(generation, fence)?;
                    value.control = Some(row.value);
                }
                UOW_NAMESPACE => {
                    if value.units.len() >= limits.max_pending_units {
                        return Err(HostProblem::ResourceExhausted);
                    }
                    let row: ObjectRow<UnitOwner> = serde_json::from_slice(&record.payload)
                        .map_err(|_| HostProblem::Malformed)?;
                    if row.schema_version != OBJECT_ROW_SCHEMA
                        || row.object_key != record.key
                        || record.key != row.value.unit.to_string()
                        || value.units.insert(row.value.unit, row.value).is_some()
                    {
                        return Err(HostProblem::Malformed);
                    }
                }
                _ => unreachable!("filtered exact owned namespaces"),
            }
        }
        if !value.units.is_empty() && value.control.is_none() {
            return Err(HostProblem::Malformed);
        }
        if let Some(control) = &value.control {
            // Every allocated owner is retained, including empty/final units.
            // No retention/removal protocol exists here. Prove the finite full
            // prefix by count plus unique positive IDs below next_unit; do not
            // iterate an untrusted counter or accept missing historical rows.
            if control.next_unit().checked_sub(1) != Some(value.units.len() as u64) {
                return Err(HostProblem::Malformed);
            }
            for unit in value.units.values() {
                unit.validate(control)?;
            }
        }
        Ok(value)
    }

    /// Produces only changed exact owner rows. Historical rows remain untouched;
    /// final owners cannot be rewritten, removed or reassigned by ordinary work.
    pub(super) fn changes(
        &self,
        control: &Control,
        units: &BTreeMap<u64, UnitOwner>,
        limits: MqLimits,
    ) -> Result<Vec<ProviderStateMutation>, HostProblem> {
        control.validate(control.generation, control.fence)?;
        if let Some(old) = &self.control {
            control.require_successor(old)?;
        }
        if units.len() > limits.max_pending_units
            || self.units.keys().any(|id| !units.contains_key(id))
        {
            return Err(HostProblem::ResourceExhausted);
        }
        for unit in units.values() {
            unit.validate(control)?;
            if let Some(old) = self.units.get(&unit.unit) {
                if old.state != UnitState::Pending && old != unit {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut allowed = old.clone();
                allowed.state = unit.state;
                allowed.queues = unit.queues.clone();
                if allowed != *unit || old.queues.iter().any(|q| !unit.queues.contains(q)) {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
        let first = self.control.as_ref().map_or(1, Control::next_unit);
        let expected = control
            .next_unit()
            .checked_sub(first)
            .ok_or(HostProblem::Malformed)?;
        if expected > limits.max_pending_units as u64
            || (first..control.next_unit()).any(|id| !units.contains_key(&id))
            || units
                .keys()
                .any(|id| !self.units.contains_key(id) && *id < first)
        {
            return Err(HostProblem::Malformed);
        }
        let mut changes = Vec::new();
        if self.control.as_ref() != Some(control) {
            changes.push(self.put(CONTROL_NAMESPACE, CONTROL_KEY, control, limits)?);
        }
        for (id, unit) in units {
            if self.units.get(id) != Some(unit) {
                changes.push(self.put(UOW_NAMESPACE, &id.to_string(), unit, limits)?);
            }
        }
        Ok(changes)
    }

    /// Pin the exact retained incarnation and every consulted current owner,
    /// including no-syncpoint/observation calls whose UOW payload is unchanged.
    /// CAS version advances preserve the exact immutable semantic provenance.
    pub(super) fn admission_changes(
        &self,
        control: &Control,
        units: &BTreeMap<u64, UnitOwner>,
        dependencies: &[u64],
        limits: MqLimits,
    ) -> Result<Vec<ProviderStateMutation>, HostProblem> {
        let mut changes = self.changes(control, units, limits)?;
        if self.control.as_ref() == Some(control) {
            changes.push(self.put(CONTROL_NAMESPACE, CONTROL_KEY, control, limits)?);
        }
        if dependencies.len() > 1 {
            return Err(HostProblem::Malformed);
        }
        for id in dependencies {
            let old = self.units.get(id).ok_or(HostProblem::Malformed)?;
            let next = units.get(id).ok_or(HostProblem::Malformed)?;
            if old == next {
                changes.push(self.put(UOW_NAMESPACE, &id.to_string(), next, limits)?);
            }
        }
        Ok(changes)
    }

    fn put<T: serde::Serialize>(
        &self,
        namespace: &str,
        key: &str,
        value: &T,
        limits: MqLimits,
    ) -> Result<ProviderStateMutation, HostProblem> {
        let expected_version = self
            .records
            .get(&(namespace.into(), key.into()))
            .map(|r| r.version);
        let version = expected_version
            .unwrap_or(0)
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or(HostProblem::ResourceExhausted)?;
        let payload = encode_object_row(key, value)?;
        if payload.len() > limits.max_state_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version,
                payload,
            },
            expected_version,
        }))
    }
}
