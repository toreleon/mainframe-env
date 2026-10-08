//! Private volatile host ownership, not attestation of application bindings.
//!
//! Only the selected service's already-admitted host dispatch may create leases.
//! A CICS child must also have passed the real host loan/frame authority; checking
//! parent fields here cannot replace that admission. No lease is serializable or
//! constructible outside this module. Call envelopes never enter this directory.
//! The service must hold its one authority lock and use its SAME handle registry
//! for retirement (via the existing pub/sub guard for reclamation). Durable UOW
//! owners, recovery fences, authorization and coordinator decisions remain with
//! their existing authorities. These volatile owners cannot identify durable work.
//! Directory counters are unique only within this host OS process. Restart must
//! advance the service's durably retained handle-registry epoch before exposure;
//! resetting these counters is not a recovery or cross-process ABA fence.
//!
//! MQ 9.4 baseline ibm-mq-9.4-mqi-2026-08-31, rows 0008/0009/0012;
//! q101760_, q101770_, q101800_: task/thread scope, process sharing and retirement.

use crate::host_context::{AttestedHostContext, decode_host_context};
use mainframe_env_execution_api::{Invocation, InvocationLimits, PrincipalId};
use mainframe_env_host_api::{HostProblem, MqHandleOwner, MqHandleRegistry, MqHostEnvironment};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

mod batch_child;
mod context;
mod native_terminal;
pub(crate) use batch_child::{BatchChildBinding, InstalledBatchRelationship, LogicalBatchOwner};
use context::ContextMode;
pub(crate) use context::DirectoryHostContext;
pub(crate) use native_terminal::TerminalRoot;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
const MAX_PROCESSES: usize = 256;
const MAX_FRAMES: usize = 4096;
const MAX_IDENTITY_BYTES: usize = 64 << 10;
const MAX_RETAINED_BYTES: usize = 16 << 20;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleLimits {
    pub(crate) processes: usize,
    pub(crate) frames: usize,
    pub(crate) retained_bytes: usize,
}
impl Default for LifecycleLimits {
    fn default() -> Self {
        Self {
            processes: MAX_PROCESSES,
            frames: MAX_FRAMES,
            retained_bytes: MAX_RETAINED_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProcessLease {
    directory: u64,
    process: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameLease {
    directory: u64,
    process: u64,
    frame: u64,
}

struct Process {
    principal: PrincipalId,
    context: AttestedHostContext,
    mode: ContextMode,
}
struct Frame {
    invocation: Invocation,
    owner: MqHandleOwner,
    bytes: usize,
    batch_origin: Option<batch_child::BatchOrigin>,
}

pub(crate) struct MqLifecycleDirectory {
    identity: u64,
    next_process: u64,
    next_frame: u64,
    limits: LifecycleLimits,
    retained_bytes: usize,
    processes: BTreeMap<u64, Process>,
    frames: BTreeMap<u64, Frame>,
}

impl MqLifecycleDirectory {
    pub(crate) fn new(limits: LifecycleLimits) -> Result<Self, HostProblem> {
        if limits.processes == 0
            || limits.processes > MAX_PROCESSES
            || limits.frames == 0
            || limits.frames > MAX_FRAMES
            || limits.retained_bytes == 0
            || limits.retained_bytes > MAX_RETAINED_BYTES
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let identity = NEXT_DIRECTORY
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| HostProblem::ResourceExhausted)?;
        Ok(Self {
            identity,
            next_process: 1,
            next_frame: 1,
            limits,
            retained_bytes: 0,
            processes: BTreeMap::new(),
            frames: BTreeMap::new(),
        })
    }

    /// Host-selected process topology only: never infer sharing from principal,
    /// a string hash, an application owner assertion, or equal binding bytes.
    pub(crate) fn mint_process(
        &mut self,
        admitted: &Invocation,
        now: u64,
    ) -> Result<ProcessLease, HostProblem> {
        self.mint_process_in_mode(admitted, now, ContextMode::Binding)
    }

    fn mint_process_in_mode(
        &mut self,
        admitted: &Invocation,
        now: u64,
        mode: ContextMode,
    ) -> Result<ProcessLease, HostProblem> {
        let (_, context) = inspect_in_mode(admitted, now, mode)?;
        if admitted.parent_execution_id.is_some() {
            return Err(HostProblem::Unauthorized);
        }
        if self.processes.len() >= self.limits.processes {
            return Err(HostProblem::ResourceExhausted);
        }
        let next = self
            .next_process
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let process = self.next_process;
        self.processes.insert(
            process,
            Process {
                principal: admitted.principal.id().clone(),
                context,
                mode,
            },
        );
        self.next_process = next;
        Ok(ProcessLease {
            directory: self.identity,
            process,
        })
    }

    pub(crate) fn bind_root(
        &mut self,
        process: ProcessLease,
        admitted: &Invocation,
        now: u64,
    ) -> Result<FrameLease, HostProblem> {
        self.bind_root_in_mode(process, admitted, now, ContextMode::Binding)
    }

    fn bind_root_in_mode(
        &mut self,
        process: ProcessLease,
        admitted: &Invocation,
        now: u64,
        mode: ContextMode,
    ) -> Result<FrameLease, HostProblem> {
        let (bytes, context) = inspect_in_mode(admitted, now, mode)?;
        self.process(process, admitted, context)?;
        if self.processes[&process.process].mode != mode {
            return Err(HostProblem::Unauthorized);
        }
        if admitted.parent_execution_id.is_some() {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(lease) = self.existing(process, admitted)? {
            return Ok(lease);
        }
        if self
            .frames
            .values()
            .any(|frame| frame.invocation.run_unit_id == admitted.run_unit_id)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let owner = MqHandleOwner {
            environment: context.environment,
            host_id: self.identity,
            process_id: process.process,
            thread_id: self.next_frame,
            task_id: self.next_frame,
            syncpoint_epoch: 1,
        };
        self.insert(process, admitted, owner, bytes)
    }

    /// Only an explicitly admitted CICS child inherits its parent task. IMS and
    /// batch subtasks are not silently merged into their parent's nonshared unit.
    pub(crate) fn bind_cics_child(
        &mut self,
        parent: FrameLease,
        child: &Invocation,
        now: u64,
    ) -> Result<FrameLease, HostProblem> {
        let (bytes, context) = inspect_in_mode(child, now, ContextMode::Binding)?;
        let parent_frame = self.frame(parent)?;
        inspect_in_mode(&parent_frame.invocation, now, ContextMode::Binding)?;
        let original = &parent_frame.invocation;
        if context.environment != MqHostEnvironment::ZosCics
            || child.parent_execution_id.as_ref() != Some(&original.execution_id)
            || child.execution_id == original.execution_id
            || child.run_unit_id != original.run_unit_id
            || child.principal != original.principal
            || child.provider_generations != original.provider_generations
            || child.cancellation != original.cancellation
            || child.cancellation_probe != original.cancellation_probe
            || child.deadline_tick > original.deadline_tick
            || !within(child.limits, original.limits)
            || child.bindings.get("cics.execution-context")
                != original.bindings.get("cics.execution-context")
            || child.bindings.get("cics.session") != original.bindings.get("cics.session")
            || decode_host_context(original)? != Some(context)
        {
            return Err(HostProblem::Unauthorized);
        }
        let owner = parent_frame.owner;
        let process = ProcessLease {
            directory: self.identity,
            process: parent.process,
        };
        self.process(process, child, context)?;
        if let Some(lease) = self.existing(process, child)? {
            return Ok(lease);
        }
        self.insert(process, child, owner, bytes)
    }

    pub(crate) fn owner_for(
        &self,
        lease: FrameLease,
        admitted: &Invocation,
        now: u64,
    ) -> Result<MqHandleOwner, HostProblem> {
        let (_, context) = inspect_in_mode(admitted, now, self.mode_for(lease))?;
        let frame = self.frame(lease)?;
        self.process(
            ProcessLease {
                directory: self.identity,
                process: lease.process,
            },
            admitted,
            context,
        )?;
        if &frame.invocation != admitted {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(frame.owner)
    }

    /// Lifetime transition only, after the owning IMS coordinator resolves work.
    /// All fallible checks precede retirement; epoch exhaustion changes neither authority.
    pub(crate) fn advance_ims_syncpoint(
        &mut self,
        lease: FrameLease,
        registry: &mut MqHandleRegistry,
    ) -> Result<MqHandleOwner, HostProblem> {
        let old = self.frame(lease)?.owner;
        if old.environment != MqHostEnvironment::ZosIms {
            return Err(HostProblem::Malformed);
        }
        let next = old
            .syncpoint_epoch
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        registry
            .end_processing_unit(old)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let frame = self
            .frames
            .get_mut(&lease.frame)
            .expect("checked frame under owned directory borrow");
        frame.owner.syncpoint_epoch = next;
        Ok(frame.owner)
    }

    /// Ending a nested frame does not end the parent task. Retire the registry's
    /// processing unit exactly when the final frame owning that unit disappears.
    pub(crate) fn retire_frame(
        &mut self,
        lease: FrameLease,
        registry: &mut MqHandleRegistry,
    ) -> Result<(), HostProblem> {
        let frame = self.frame(lease)?;
        let owner = frame.owner;
        let bytes = frame.bytes;
        let final_frame = !self
            .frames
            .iter()
            .any(|(id, other)| *id != lease.frame && other.owner == owner);
        if final_frame {
            registry
                .end_processing_unit(owner)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
        }
        self.frames.remove(&lease.frame);
        self.retained_bytes -= bytes;
        Ok(())
    }

    /// Must follow frame retirement and the existing durable UOW decision path.
    /// This removes remaining shared volatile handles, not messages or UOW rows.
    pub(crate) fn retire_process(
        &mut self,
        lease: ProcessLease,
        registry: &mut MqHandleRegistry,
    ) -> Result<(), HostProblem> {
        if lease.directory != self.identity {
            return Err(HostProblem::Unauthorized);
        }
        let process = self
            .processes
            .get(&lease.process)
            .ok_or(HostProblem::Unauthorized)?;
        if self
            .frames
            .values()
            .any(|frame| frame.owner.process_id == lease.process)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let owner = MqHandleOwner {
            environment: process.context.environment,
            host_id: self.identity,
            process_id: lease.process,
            thread_id: 1,
            task_id: 1,
            syncpoint_epoch: 1,
        };
        registry
            .end_process(owner)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.processes.remove(&lease.process);
        Ok(())
    }

    fn process(
        &self,
        lease: ProcessLease,
        invocation: &Invocation,
        context: AttestedHostContext,
    ) -> Result<(), HostProblem> {
        if lease.directory != self.identity {
            return Err(HostProblem::Unauthorized);
        }
        let process = self
            .processes
            .get(&lease.process)
            .ok_or(HostProblem::Unauthorized)?;
        if process.principal != *invocation.principal.id() || process.context != context {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
    fn frame(&self, lease: FrameLease) -> Result<&Frame, HostProblem> {
        if lease.directory != self.identity || !self.processes.contains_key(&lease.process) {
            return Err(HostProblem::Unauthorized);
        }
        self.frames
            .get(&lease.frame)
            .filter(|frame| frame.owner.process_id == lease.process)
            .ok_or(HostProblem::Unauthorized)
    }
    fn existing(
        &self,
        process: ProcessLease,
        invocation: &Invocation,
    ) -> Result<Option<FrameLease>, HostProblem> {
        for (frame, existing) in &self.frames {
            if existing.invocation.execution_id == invocation.execution_id {
                if existing.owner.process_id != process.process
                    || &existing.invocation != invocation
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return Ok(Some(FrameLease {
                    directory: self.identity,
                    process: process.process,
                    frame: *frame,
                }));
            }
        }
        Ok(None)
    }
    fn insert(
        &mut self,
        process: ProcessLease,
        invocation: &Invocation,
        owner: MqHandleOwner,
        bytes: usize,
    ) -> Result<FrameLease, HostProblem> {
        let origin = if invocation.parent_execution_id.is_none()
            && owner.environment == MqHostEnvironment::ZosBatch
        {
            Some(batch_child::BatchOrigin::root(invocation))
        } else {
            None
        };
        self.insert_with_origin(process, invocation, owner, bytes, origin)
    }

    fn insert_with_origin(
        &mut self,
        process: ProcessLease,
        invocation: &Invocation,
        owner: MqHandleOwner,
        bytes: usize,
        batch_origin: Option<batch_child::BatchOrigin>,
    ) -> Result<FrameLease, HostProblem> {
        let bytes = bytes
            .checked_add(batch_origin.as_ref().map_or(0, |origin| origin.bytes()))
            .ok_or(HostProblem::ResourceExhausted)?;
        let retained = self
            .retained_bytes
            .checked_add(bytes)
            .filter(|size| *size <= self.limits.retained_bytes)
            .ok_or(HostProblem::ResourceExhausted)?;
        let next = self
            .next_frame
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        if self.frames.len() >= self.limits.frames {
            return Err(HostProblem::ResourceExhausted);
        }
        let frame = self.next_frame;
        self.frames.insert(
            frame,
            Frame {
                invocation: invocation.clone(),
                owner,
                bytes,
                batch_origin,
            },
        );
        self.next_frame = next;
        self.retained_bytes = retained;
        Ok(FrameLease {
            directory: self.identity,
            process: process.process,
            frame,
        })
    }
}

fn within(
    child: mainframe_env_execution_api::ResourceLimits,
    parent: mainframe_env_execution_api::ResourceLimits,
) -> bool {
    child.max_steps <= parent.max_steps
        && child.max_storage_bytes <= parent.max_storage_bytes
        && child.max_output_bytes <= parent.max_output_bytes
        && child.max_frames <= parent.max_frames
        && child.max_effects <= parent.max_effects
        && child.max_events <= parent.max_events
}

fn inspect_in_mode(
    invocation: &Invocation,
    now: u64,
    mode: ContextMode,
) -> Result<(usize, AttestedHostContext), HostProblem> {
    if invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if now == 0
        || now == u64::MAX
        || invocation.deadline_tick == 0
        || invocation.deadline_tick == u64::MAX
        || invocation.attempt == 0
        || invocation.limits.validate().is_err()
    {
        return Err(HostProblem::Malformed);
    }
    if now >= invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let limits = InvocationLimits::default();
    if invocation.bindings.len() > limits.max_bindings
        || invocation.principal.grants().len() > limits.max_capabilities
        || invocation.provider_generations.len() > limits.max_capabilities
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bytes = 0usize;
    let mut count = |text: &[u8], ceiling: usize| -> Result<(), HostProblem> {
        if text.len() > ceiling {
            return Err(HostProblem::ResourceExhausted);
        }
        bytes = bytes
            .checked_add(text.len())
            .filter(|n| *n <= MAX_IDENTITY_BYTES)
            .ok_or(HostProblem::ResourceExhausted)?;
        Ok(())
    };
    for identity in [
        invocation.request_id.as_str(),
        invocation.execution_id.as_str(),
        invocation.run_unit_id.as_str(),
        invocation.selector.as_str(),
        invocation.artifact.as_str(),
        invocation.principal.id().as_str(),
        invocation.trace_id.as_str(),
        invocation.idempotency_key.as_str(),
        &invocation.audit_correlation,
    ] {
        count(identity.as_bytes(), limits.max_identity_bytes)?;
    }
    if let Some(parent) = &invocation.parent_execution_id {
        count(parent.as_str().as_bytes(), limits.max_identity_bytes)?;
    }
    for grant in invocation.principal.grants() {
        count(grant.as_str().as_bytes(), limits.max_identity_bytes)?;
    }
    for (capability, generation) in &invocation.provider_generations {
        if generation.is_empty() || !invocation.principal.has_grant(capability) {
            return Err(HostProblem::Malformed);
        }
        count(capability.as_str().as_bytes(), limits.max_identity_bytes)?;
        count(generation.as_bytes(), limits.max_identity_bytes)?;
    }
    for (key, binding) in &invocation.bindings {
        if key.is_empty() {
            return Err(HostProblem::Malformed);
        }
        count(key.as_bytes(), limits.max_identity_bytes)?;
        count(binding.schema().as_bytes(), limits.max_identity_bytes)?;
        count(binding.bytes(), limits.max_payload_bytes)?;
    }
    let context = mode.resolve(invocation)?;
    Ok((bytes, context))
}

#[cfg(test)]
mod tests;
