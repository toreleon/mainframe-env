//! Narrow helpers for the privileged Rust facet, under the existing sole mutex.
use super::*;

impl MqService {
    #[cfg(test)]
    pub(crate) fn trusted_batch_test_reply_uncertainty(&self) {
        self.unknown_after_persist.store(true, Ordering::SeqCst);
    }
    #[cfg(test)]
    pub(crate) fn trusted_batch_test_depth(&self, queue: &crate::MqObjectName) -> usize {
        let guard = self.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(state) = &*guard else {
            panic!("rich fixture")
        };
        state.delivery.depth(queue).unwrap()
    }
    pub(crate) fn require_trusted_batch_rich(&self) -> Result<(), HostProblem> {
        let guard = self.lock_selected()?;
        if matches!(&*guard, rich_state::StoredAuthority::Rich(_)) {
            Ok(())
        } else {
            Err(HostProblem::Unsupported)
        }
    }

    pub(crate) fn prepare_trusted_batch_root(
        &self,
        original: &Invocation,
    ) -> Result<FrameLease, HostProblem> {
        let context = crate::host_context::AttestedHostContext {
            environment: MqHostEnvironment::ZosBatch,
            owner: MqSyncpointOwner::QueueManager,
        };
        crate::host_context::explicit_batch_context(original, context)?;
        if original.parent_execution_id.is_some() {
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
        let runtime = state.runtime.as_mut().ok_or(HostProblem::Unauthorized)?;
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        let process = runtime
            .directory
            .mint_process_explicit(original, context, now)?;
        match runtime.directory.bind_root_explicit(process, original, now) {
            Ok(frame) => Ok(frame),
            Err(problem) => {
                // Only this fresh empty process, never an existing task end.
                // Reclamation refuses referenced processes; no handles/durable
                // work can exist for an unpublished failed root preparation.
                runtime
                    .directory
                    .retire_process(process, &mut runtime.handles.handles_mut())?;
                Err(problem)
            }
        }
    }

    pub(crate) fn fence_trusted_batch(&self) -> Result<(), HostProblem> {
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        state
            .runtime
            .as_mut()
            .ok_or(HostProblem::Unauthorized)?
            .fenced = true;
        Ok(())
    }
}
