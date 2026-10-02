//! Separately retained local MQ UOW provenance. Volatile handles are not owners.
//!
//! Allocation is from one persisted, CAS-protected service control row. The
//! caller's Local.unit is only an assertion against the issued connection's
//! current retained unit. This does not grant shared-participant capability.

use crate::mqi_lifecycle::LogicalBatchOwner;
use mainframe_env_execution_api::{ExecutionId, InvocationLimits, PrincipalId, RunUnitId};
use mainframe_env_host_api::HostProblem;
use serde::{Deserialize, Serialize};

pub(super) const CONTROL_NAMESPACE: &str = "mq-selected-v1-control";
pub(super) const CONTROL_KEY: &str = "state";
pub(super) const UOW_NAMESPACE: &str = "mq-selected-v1-uow-owner";
const CONTROL_SCHEMA: &str = "mainframe-env.mq-selected-control@1";
const UOW_SCHEMA: &str = "mainframe-env.mq-selected-uow-owner@1";

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(in crate::service) struct Control {
    schema_version: String,
    pub(super) generation: u64,
    pub(super) fence: u64,
    pub(super) registry_epoch: u64,
    next_unit: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum UnitState {
    Pending,
    Committed,
    RolledBack,
}

/// Connection identity is the original CONNECT effect key, not an encoded
/// opaque HCONN or a directory number. Connection lookup still requires the
/// exact live registry token and its service-owned binding under the mutex.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct UnitOwner {
    schema_version: String,
    coordinator: String,
    pub(super) unit: u64,
    pub(super) connection_key: String,
    execution: String,
    run: String,
    principal: String,
    pub(super) generation: u64,
    pub(super) fence: u64,
    pub(super) registry_epoch: u64,
    pub(super) state: UnitState,
    pub(super) queues: Vec<String>,
}

fn positive(value: u64) -> bool {
    value > 0 && value <= i64::MAX as u64
}

impl Control {
    pub(super) fn require_successor(&self, old: &Self) -> Result<(), HostProblem> {
        if self.generation != old.generation
            || self.fence != old.fence
            || self.next_unit < old.next_unit
            || (self.registry_epoch != old.registry_epoch
                && old.registry_epoch.checked_add(1) != Some(self.registry_epoch))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    pub(super) fn next_unit(&self) -> u64 {
        self.next_unit
    }

    /// Only the selected service's explicit first incarnation publication may
    /// create this row. Opening/reading never silently initializes authority.
    pub(super) fn initial(generation: u64, fence: u64) -> Result<Self, HostProblem> {
        let value = Self {
            schema_version: CONTROL_SCHEMA.into(),
            generation,
            fence,
            registry_epoch: 1,
            next_unit: 1,
        };
        value.validate(generation, fence)?;
        Ok(value)
    }

    /// A cold runtime must publish this checked advance before exposing any
    /// new token. No existing UOW is reassigned, restored or decided here.
    pub(super) fn next_incarnation(&self) -> Result<Self, HostProblem> {
        let mut next = self.clone();
        next.registry_epoch = next
            .registry_epoch
            .checked_add(1)
            .filter(|v| positive(*v))
            .ok_or(HostProblem::ResourceExhausted)?;
        Ok(next)
    }

    pub(super) fn validate(&self, generation: u64, fence: u64) -> Result<(), HostProblem> {
        if self.schema_version != CONTROL_SCHEMA
            || self.generation != generation
            || self.fence != fence
            || !positive(generation)
            || !positive(fence)
            || !positive(self.registry_epoch)
            || !positive(self.next_unit)
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }

    pub(super) fn allocate(
        &mut self,
        logical: &LogicalBatchOwner,
        connection_key: &str,
    ) -> Result<UnitOwner, HostProblem> {
        let next = self
            .next_unit
            .checked_add(1)
            .filter(|v| positive(*v))
            .ok_or(HostProblem::ResourceExhausted)?;
        let owner = UnitOwner {
            schema_version: UOW_SCHEMA.into(),
            coordinator: "queue-manager-local".into(),
            unit: self.next_unit,
            connection_key: connection_key.into(),
            execution: logical.execution().into(),
            run: logical.run().into(),
            principal: logical.principal().into(),
            generation: self.generation,
            fence: self.fence,
            registry_epoch: self.registry_epoch,
            state: UnitState::Pending,
            queues: Vec::new(),
        };
        owner.validate(&Self {
            next_unit: next,
            ..self.clone()
        })?;
        self.next_unit = next;
        Ok(owner)
    }
}

impl UnitOwner {
    pub(super) fn validate(&self, control: &Control) -> Result<(), HostProblem> {
        let limits = InvocationLimits::default();
        if self.schema_version != UOW_SCHEMA
            || self.coordinator != "queue-manager-local"
            || !positive(self.unit)
            || self.unit >= control.next_unit
            || self.generation != control.generation
            || self.fence != control.fence
            || !positive(self.registry_epoch)
            || self.registry_epoch > control.registry_epoch
            || self.queues.len() > 256
        {
            return Err(HostProblem::Malformed);
        }
        ExecutionId::new(&self.execution, limits).map_err(|_| HostProblem::Malformed)?;
        RunUnitId::new(&self.run, limits).map_err(|_| HostProblem::Malformed)?;
        PrincipalId::new(&self.principal, limits).map_err(|_| HostProblem::Malformed)?;
        mainframe_env_execution_api::IdempotencyKey::new(&self.connection_key, limits)
            .map_err(|_| HostProblem::Malformed)?;
        for (index, name) in self.queues.iter().enumerate() {
            crate::MqObjectName::new(name).map_err(|_| HostProblem::Malformed)?;
            if index > 0 && self.queues[index - 1] >= *name {
                return Err(HostProblem::Malformed);
            }
        }
        Ok(())
    }

    pub(super) fn require_owner(
        &self,
        logical: &LogicalBatchOwner,
        connection_key: &str,
        control: &Control,
        asserted_unit: u64,
    ) -> Result<(), HostProblem> {
        self.validate(control)?;
        if self.execution != logical.execution()
            || self.run != logical.run()
            || self.principal != logical.principal()
            || self.connection_key != connection_key
            || self.unit != asserted_unit
            || self.registry_epoch != control.registry_epoch
            || self.state != UnitState::Pending
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    pub(super) fn touch_queue(&mut self, name: &str) -> Result<(), HostProblem> {
        crate::MqObjectName::new(name).map_err(|_| HostProblem::Malformed)?;
        match self.queues.binary_search_by(|v| v.as_str().cmp(name)) {
            Ok(_) => Ok(()),
            Err(position) if self.queues.len() < 256 => {
                self.queues.insert(position, name.into());
                Ok(())
            }
            Err(_) => Err(HostProblem::ResourceExhausted),
        }
    }

    pub(super) fn finalize(&mut self, commit: bool) -> Result<(), HostProblem> {
        if self.state != UnitState::Pending {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.state = if commit {
            UnitState::Committed
        } else {
            UnitState::RolledBack
        };
        Ok(())
    }
}
