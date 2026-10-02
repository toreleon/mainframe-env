//! Private bridge for an independently admitted installed SAME TASK CALL.
//! No application binding, matching ID or profile can establish host topology.
//! A future public bridge must consume the real sealed host admission capability.

use super::*;
use crate::mqi_lifecycle::{BatchChildBinding, InstalledBatchRelationship};

impl MqService {
    /// Preconditions: the trusted host independently admitted both Invocations,
    /// their original CALL and SAME TASK relationship against this physical
    /// service/store. This checked directory method is not that attestation.
    pub(crate) fn prepare_selected_batch_child(
        &self,
        parent: FrameLease,
        admitted_parent: &Invocation,
        child: &Invocation,
        relationship: InstalledBatchRelationship,
    ) -> Result<BatchChildBinding, HostProblem> {
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let runtime = state.runtime.as_mut().ok_or(HostProblem::Unauthorized)?;
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        runtime.directory.bind_installed_batch_child(
            parent,
            admitted_parent,
            child,
            relationship,
            now,
        )
    }

    /// Read-only owner projection before constructing a new original effect.
    /// Never rewrites an envelope behind an already pending occurrence.
    pub(crate) fn selected_batch_owner(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
    ) -> Result<MqHandleOwner, HostProblem> {
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let runtime = state.runtime.as_mut().ok_or(HostProblem::Unauthorized)?;
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        Ok(runtime
            .directory
            .logical_batch_owner(frame, invocation, now)?
            .owner())
    }

    /// Explicit setup rollback only; does not release handles or decide UOWs.
    pub(crate) fn abort_selected_batch_child(
        &self,
        binding: BatchChildBinding,
    ) -> Result<(), HostProblem> {
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        state
            .runtime
            .as_mut()
            .ok_or(HostProblem::Unauthorized)?
            .directory
            .abort_batch_child(binding)
    }

    /// Normal child CALL return removes only a nonfinal volatile frame. Raw
    /// abnormal/unknown outcomes and final task end need the real host recovery
    /// authority; this method is neither implicit MQDISC nor a durable decision.
    pub(crate) fn return_selected_batch_child(
        &self,
        frame: FrameLease,
        child: &Invocation,
    ) -> Result<(), HostProblem> {
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        state
            .runtime
            .as_mut()
            .ok_or(HostProblem::Unauthorized)?
            .directory
            .return_batch_child(frame, child)
    }
}
