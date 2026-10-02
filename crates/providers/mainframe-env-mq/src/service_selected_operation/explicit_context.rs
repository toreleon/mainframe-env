//! Private trusted embedding configuration; no public attestation or fallback.
use super::*;
use crate::host_context::{AttestedHostContext, explicit_batch_context};

impl MqService {
    /// The actual host independently admitted the ORIGINAL Invocation and chose
    /// ordinary batch topology/context against this same physical service/store.
    /// Context is configuration, never inferred from bindings or reconstructed IDs.
    pub(crate) fn mint_selected_process_explicit(
        &self,
        invocation: &Invocation,
        context: AttestedHostContext,
    ) -> Result<ProcessLease, HostProblem> {
        explicit_batch_context(invocation, context)?;
        if invocation.parent_execution_id.is_some() {
            return Err(HostProblem::Unauthorized);
        }
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        if state.runtime.is_none() {
            state.runtime = Some(SelectedRuntime::new(state, self.limits)?);
        }
        let runtime = state
            .runtime
            .as_mut()
            .expect("runtime initialized under sole authority");
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        runtime
            .directory
            .mint_process_explicit(invocation, context, now)
    }

    /// Only an explicit-mode opaque process lease can bind this root. Ordinary
    /// binding-mode leases cannot be upgraded, and no context is substituted.
    pub(crate) fn bind_selected_root_explicit(
        &self,
        process: ProcessLease,
        invocation: &Invocation,
    ) -> Result<(FrameLease, MqHandleOwner), HostProblem> {
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        let runtime = state.runtime.as_mut().ok_or(HostProblem::Unauthorized)?;
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        let frame = runtime
            .directory
            .bind_root_explicit(process, invocation, now)?;
        Ok((frame, runtime.directory.owner_for(frame, invocation, now)?))
    }
}
