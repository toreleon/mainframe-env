//! Volatile same-level admission within the existing synchronous program loan.
use super::*;
use mainframe_env_host_api::ProgramLinkSelection;

struct ReplacementGuard<'a> {
    service: &'a CicsService,
    session: String,
    known: bool,
    _thread_confined: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for ReplacementGuard<'_> {
    fn drop(&mut self) {
        if !self.known
            && let Ok(mut state) = self.service.lock()
        {
            state
                .task_dispatch
                .uncertain_sessions
                .insert(self.session.clone());
        }
    }
}

fn preserves_controls(source: &Invocation, target: &Invocation) -> bool {
    target.execution_id != source.execution_id
        && target.parent_execution_id.as_ref() == Some(&source.execution_id)
        && target.run_unit_id == source.run_unit_id
        && target.principal == source.principal
        && target.service_class == source.service_class
        && target.priority == source.priority
        && target.provider_generations == source.provider_generations
        && target.audit_correlation == source.audit_correlation
        && target.cancellation == source.cancellation
        && target.cancellation_probe == source.cancellation_probe
        && target.deadline_tick <= source.deadline_tick
        && target.limits.max_frames <= source.limits.max_frames
        && target.limits.max_steps <= source.limits.max_steps
        && target.limits.max_storage_bytes <= source.limits.max_storage_bytes
        && target.limits.max_output_bytes <= source.limits.max_output_bytes
        && target.limits.max_effects <= source.limits.max_effects
        && target.limits.max_events <= source.limits.max_events
        && target.bindings.get("cics.execution-context")
            == source.bindings.get("cics.execution-context")
        && target.bindings.get("cics.session") == source.bindings.get("cics.session")
        && target.bindings.len() <= InvocationLimits::default().max_bindings
}

impl CicsService {
    /// Run an independently attested replacement within the current synchronous loan.
    /// The embedding must prove the original canonical Transfer, pending CALL,
    /// immutable executable, instance CAS ownership and durable handoff before
    /// executing. This volatile admission cannot reconstruct a cold lease or
    /// confer terminal proof. Its closure must return only a known terminal
    /// result; an unresolved result fences the task instead of restoring source.
    pub fn with_same_level_program_frame(
        &self,
        source: &Invocation,
        target: &Invocation,
        selection: &ProgramLinkSelection,
        execute: impl FnOnce() -> Result<BoundedPayload, HostProblem>,
    ) -> Result<BoundedPayload, HostProblem> {
        if !preserves_controls(source, target) || target.artifact != selection.artifact {
            return Err(HostProblem::Unauthorized);
        }
        if source.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        let program =
            super::super::task_context::current_program(target).ok_or(HostProblem::Malformed)?;
        super::super::program_control::validate_frozen_selection(self, &program, selection)?;
        let session = {
            let mut state = self.lock()?;
            let task = state
                .runs
                .get(&source.run_unit_id)
                .ok_or(HostProblem::Unauthorized)?;
            state
                .task_dispatch
                .require_available_session(&task.session)?;
            if task.current_program.effect_invocation != *source {
                return Err(HostProblem::Unauthorized);
            }
            let session = task.session.clone();
            let claim = state
                .task_dispatch
                .claims
                .get_mut(&source.run_unit_id)
                .ok_or(HostProblem::Unauthorized)?;
            if claim.thread != std::thread::current().id() || claim.commands != claim.loans.len() {
                return Err(HostProblem::Unauthorized);
            }
            let loan = claim.loans.last_mut().ok_or(HostProblem::Unauthorized)?;
            if loan.actor.as_ref() != Some(source) {
                return Err(HostProblem::Unauthorized);
            }
            loan.parent = source.clone();
            loan.program = program.clone();
            loan.artifact = Some(selection.artifact.clone());
            loan.actor = Some(target.clone());
            let task = state
                .runs
                .get_mut(&source.run_unit_id)
                .expect("validated task");
            task.current_program.effect_invocation = target.clone();
            task.current_program.parent_execution_id = target.parent_execution_id.clone();
            task.current_program.current = Some(program);
            task.current_program.channel = super::super::task_context::current_channel(target);
            task.current_program.program_occurrence = 0;
            task.current_program.initial_entry = false;
            session
        };
        let mut guard = ReplacementGuard {
            service: self,
            session,
            known: false,
            _thread_confined: std::marker::PhantomData,
        };
        let result = execute();
        guard.known = result.is_ok()
            || (matches!(&result,
            Err(HostProblem::Condition { name, response: -1, response2: 0 })
                if name == "INSTALLED-CALL-ABEND")
                && self
                    .lock()
                    .map_err(|_| HostProblem::UnknownOutcome)?
                    .runs
                    .get(&source.run_unit_id)
                    .is_some_and(|task| task.program_abend.is_some()));
        if !guard.known {
            return Err(HostProblem::UnknownOutcome);
        }
        result
    }
}

#[cfg(test)]
mod tests;
