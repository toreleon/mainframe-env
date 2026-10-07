//! Frozen private context plane. No Serde/public construction or host attestation.
use super::*;
use crate::host_context::explicit_batch_context;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ContextMode {
    Binding,
    ExplicitBatch,
}
impl ContextMode {
    pub(super) fn resolve(
        self,
        invocation: &Invocation,
    ) -> Result<AttestedHostContext, HostProblem> {
        match self {
            Self::Binding => decode_host_context(invocation)?.ok_or(HostProblem::Malformed),
            Self::ExplicitBatch => explicit_batch_context(invocation, batch()),
        }
    }
}
fn batch() -> AttestedHostContext {
    AttestedHostContext {
        environment: MqHostEnvironment::ZosBatch,
        owner: mainframe_env_host_api::MqSyncpointOwner::QueueManager,
    }
}

/// Issued only after bounded full frozen Invocation/owner validation. Its exact
/// original snapshot is bounded by inspect's existing 64KiB aggregate budget.
/// Keep within the service's same locked dispatch, never serialize or retain it.
pub(crate) struct DirectoryHostContext {
    invocation: Invocation,
    owner: MqHandleOwner,
    mode: ContextMode,
}
impl DirectoryHostContext {
    pub(crate) fn require(
        &self,
        invocation: &Invocation,
        owner: MqHandleOwner,
    ) -> Result<AttestedHostContext, HostProblem> {
        if invocation != &self.invocation || owner != self.owner {
            return Err(HostProblem::Unauthorized);
        }
        self.mode.resolve(invocation)
    }
}

impl MqLifecycleDirectory {
    /// Already-admitted trusted host selects ordinary batch topology/context;
    /// this private parameter is not an application attestation constructor.
    pub(crate) fn mint_process_explicit(
        &mut self,
        invocation: &Invocation,
        context: AttestedHostContext,
        now: u64,
    ) -> Result<ProcessLease, HostProblem> {
        explicit_batch_context(invocation, context)?;
        self.mint_process_in_mode(invocation, now, ContextMode::ExplicitBatch)
    }

    pub(crate) fn bind_root_explicit(
        &mut self,
        process: ProcessLease,
        invocation: &Invocation,
        now: u64,
    ) -> Result<FrameLease, HostProblem> {
        self.bind_root_in_mode(process, invocation, now, ContextMode::ExplicitBatch)
    }

    pub(super) fn mode_for(&self, frame: FrameLease) -> ContextMode {
        if frame.directory == self.identity {
            self.processes
                .get(&frame.process)
                .map(|p| p.mode)
                .unwrap_or(ContextMode::Binding)
        } else {
            ContextMode::Binding
        }
    }

    pub(crate) fn context_for(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        now: u64,
    ) -> Result<DirectoryHostContext, HostProblem> {
        let owner = self.owner_for(frame, invocation, now)?;
        Ok(DirectoryHostContext {
            invocation: invocation.clone(),
            owner,
            mode: self.mode_for(frame),
        })
    }
}
