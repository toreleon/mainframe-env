//! Checked volatile lineage for an independently admitted ordinary SAME TASK
//! CALL. The selector below records the trusted host's topology decision; this
//! module does not attest that decision from Invocation or application bindings.
//! It creates no durable owner, task-end decision or public host capability.

use super::*;
use mainframe_env_execution_api::{ExecutionId, RunUnitId};
use mainframe_env_host_api::MqSyncpointOwner;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstalledBatchRelationship {
    SameTaskCall,
    SeparateSubtask,
}

/// No numeric/Serde constructor. Only a successful directory preparation issues
/// this token. Repeated preparation never authorizes aborting an existing frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BatchChildBinding {
    frame: FrameLease,
    created: bool,
}
impl BatchChildBinding {
    pub(crate) fn frame(self) -> FrameLease {
        self.frame
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct BatchOrigin {
    execution: ExecutionId,
    run: RunUnitId,
    principal: PrincipalId,
}
impl BatchOrigin {
    pub(super) fn root(invocation: &Invocation) -> Self {
        Self {
            execution: invocation.execution_id.clone(),
            run: invocation.run_unit_id.clone(),
            principal: invocation.principal.id().clone(),
        }
    }
    pub(super) fn bytes(&self) -> usize {
        self.execution.as_str().len() + self.run.as_str().len() + self.principal.as_str().len()
    }
}

/// Frozen origin copied only through the checked directory lineage. It cannot
/// be reconstructed from a CONNECT receipt or equal context/owner assertions.
/// Callers must obtain it afresh under the same service authority lock.
pub(crate) struct LogicalBatchOwner {
    origin: BatchOrigin,
    owner: MqHandleOwner,
    child: bool,
}
impl LogicalBatchOwner {
    pub(crate) fn execution(&self) -> &str {
        self.origin.execution.as_str()
    }
    pub(crate) fn run(&self) -> &str {
        self.origin.run.as_str()
    }
    pub(crate) fn principal(&self) -> &str {
        self.origin.principal.as_str()
    }
    pub(crate) fn owner(&self) -> MqHandleOwner {
        self.owner
    }
    pub(crate) fn is_child(&self) -> bool {
        self.child
    }
}

impl MqLifecycleDirectory {
    pub(crate) fn bind_installed_batch_child(
        &mut self,
        parent: FrameLease,
        admitted_parent: &Invocation,
        child: &Invocation,
        relationship: InstalledBatchRelationship,
        now: u64,
    ) -> Result<BatchChildBinding, HostProblem> {
        if relationship != InstalledBatchRelationship::SameTaskCall {
            return Err(HostProblem::Unsupported);
        }
        let owner = self.owner_for(parent, admitted_parent, now)?;
        let (bytes, context) = inspect(child, now)?;
        let parent_context = decode_host_context(admitted_parent)?;
        if context.environment != MqHostEnvironment::ZosBatch
            || context.owner != MqSyncpointOwner::QueueManager
            || parent_context != Some(context)
            || child.parent_execution_id.as_ref() != Some(&admitted_parent.execution_id)
            || child.execution_id == admitted_parent.execution_id
            || child.run_unit_id != admitted_parent.run_unit_id
            || child.principal != admitted_parent.principal
            || child.provider_generations != admitted_parent.provider_generations
            || child.attempt != admitted_parent.attempt
            || child.cancellation != admitted_parent.cancellation
            || child.cancellation_probe.is_none()
            || child.cancellation_probe != admitted_parent.cancellation_probe
            || child.deadline_tick > admitted_parent.deadline_tick
            || !within(child.limits, admitted_parent.limits)
        {
            return Err(HostProblem::Unauthorized);
        }
        let origin = self
            .frame(parent)?
            .batch_origin
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        let process = ProcessLease {
            directory: self.identity,
            process: parent.process,
        };
        self.process(process, child, context)?;
        if let Some(frame) = self.existing(process, child)? {
            let existing = self.frame(frame)?;
            if existing.owner != owner || existing.batch_origin.as_ref() != Some(origin) {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(BatchChildBinding {
                frame,
                created: false,
            });
        }
        // All fallible bounds precede cloning/inserting the new frame. The
        // origin is bounded by the already inspected identity lengths.
        let retained = bytes
            .checked_add(origin.bytes())
            .and_then(|n| self.retained_bytes.checked_add(n))
            .filter(|n| *n <= self.limits.retained_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
        if self.frames.len() >= self.limits.frames || self.next_frame.checked_add(1).is_none() {
            return Err(HostProblem::ResourceExhausted);
        }
        let origin = origin.clone();
        let frame = self.insert_with_origin(process, child, owner, bytes, Some(origin))?;
        debug_assert_eq!(self.retained_bytes, retained);
        Ok(BatchChildBinding {
            frame,
            created: true,
        })
    }

    pub(crate) fn logical_batch_owner(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        now: u64,
    ) -> Result<LogicalBatchOwner, HostProblem> {
        let owner = self.owner_for(frame, invocation, now)?;
        let origin = self
            .frame(frame)?
            .batch_origin
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        Ok(LogicalBatchOwner {
            origin: origin.clone(),
            owner,
            child: invocation.parent_execution_id.is_some(),
        })
    }

    /// Preparation rollback touches only its newly created frame, not handles,
    /// queues, UOWs, parent references or counters. No Drop cleanup is installed.
    pub(crate) fn abort_batch_child(
        &mut self,
        binding: BatchChildBinding,
    ) -> Result<(), HostProblem> {
        let frame = self.frame(binding.frame)?;
        if frame.batch_origin.is_none() || frame.invocation.parent_execution_id.is_none() {
            return Err(HostProblem::Unauthorized);
        }
        if !binding.created {
            return Ok(());
        }
        if self.frames.values().any(|other| {
            other.invocation.parent_execution_id.as_ref() == Some(&frame.invocation.execution_id)
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let bytes = frame.bytes;
        self.frames.remove(&binding.frame.frame);
        self.retained_bytes -= bytes;
        Ok(())
    }

    /// Ordinary CALL return is not application/task end or MQDISC. Cleanup of a
    /// cancelled/expired child is allowed, but never decides durable work. The
    /// final processing-unit reference requires the separate task-end authority.
    pub(crate) fn return_batch_child(
        &mut self,
        lease: FrameLease,
        invocation: &Invocation,
    ) -> Result<(), HostProblem> {
        let frame = self.frame(lease)?;
        if &frame.invocation != invocation
            || frame.batch_origin.is_none()
            || invocation.parent_execution_id.is_none()
        {
            return Err(HostProblem::Unauthorized);
        }
        if !self
            .frames
            .iter()
            .any(|(id, other)| *id != lease.frame && other.owner == frame.owner)
        {
            return Err(HostProblem::Unsupported);
        }
        let bytes = frame.bytes;
        self.frames.remove(&lease.frame);
        self.retained_bytes -= bytes;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
