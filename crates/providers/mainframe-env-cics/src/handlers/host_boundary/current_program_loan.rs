//! Borrow the actual current CICS task at an exclusive external dispatch boundary.
//! This does not attest compilation, coordinator Intent, MQ admission or settlement.
use super::*;
use std::marker::PhantomData;
use std::rc::Rc;

mod initial_entry;

#[cfg(test)]
mod tests;

// A deterministic test observation of the actual exit boundary. It grants no
// authority and is absent from non-test builds.
#[cfg(test)]
type BeforeRestoreObserver = Box<dyn FnOnce(&CicsService, &Invocation)>;
#[cfg(test)]
std::thread_local! {
    static BEFORE_RESTORE: std::cell::RefCell<Option<BeforeRestoreObserver>> =
        std::cell::RefCell::new(None);
}

/// A current task/program borrowed while its original command lease is held.
///
/// There is no constructor, Clone, Debug or serialization implementation. The
/// callback cannot return this loan or keep it after the command is restored.
/// This loan attests the CICS task custody only. An installed host must still
/// obtain actual compiled-artifact, coordinator Intent, store and MQ authority.
/// No CALL completion, RETURN, commit, backout or recovery follows from it.
pub struct CicsCurrentProgramLoan<'a> {
    service: &'a CicsService,
    run: &'a Run,
    _thread_confined: PhantomData<Rc<()>>,
}

impl CicsCurrentProgramLoan<'_> {
    /// The exact current program actor, never a rewritten terminal snapshot.
    pub fn invocation(&self) -> &Invocation {
        &self.run.current_program.effect_invocation
    }

    /// The actual registered task session while its exclusive lease is held.
    pub fn session(&self) -> &str {
        &self.run.session
    }

    /// Match the actual retained adapters; equal rows or artifact bytes are insufficient.
    /// This observation grants no independent store or scheduling authority.
    pub fn require_same_installed_storage(
        &self,
        store: &Arc<dyn mainframe_env_store_api::PlatformStore>,
        artifacts: &Arc<dyn mainframe_env_store_api::ArtifactStore>,
    ) -> Result<(), HostProblem> {
        let provider: Arc<dyn mainframe_env_store_api::ProviderStateStore> = store.clone();
        if !Arc::ptr_eq(&self.service.store, &provider)
            || self
                .service
                .artifacts
                .get()
                .is_none_or(|actual| !Arc::ptr_eq(actual, artifacts))
        {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }

    /// Read the current immutable local selection while this actual task is held.
    /// The caller must compare it to its frozen admitted compiled selection.
    pub fn installed_program_selection(&self) -> Result<ProgramLinkSelection, HostProblem> {
        let program = self
            .run
            .current_program
            .current
            .as_ref()
            .ok_or(HostProblem::Unauthorized)?;
        super::super::program_control::local_link_selection(self.service, program)?
            .filter(|selection| selection.artifact == self.invocation().artifact)
            .ok_or(HostProblem::Unauthorized)
    }

    /// Validate the borrowed actor and original same-thread command custody.
    ///
    /// A tick is an offered control observation, not a clock authority. The
    /// embedding must supply it from the actual current coordinator control.
    pub fn require_current(&self, original: &Invocation, now_tick: u64) -> Result<(), HostProblem> {
        require_frame(self.run, original, now_tick)?;
        let state = self.service.lock()?;
        state
            .task_dispatch
            .require_available_session(&self.run.session)?;
        let claim = state
            .task_dispatch
            .claims
            .get(&original.run_unit_id)
            .ok_or(HostProblem::Unauthorized)?;
        if claim.thread != std::thread::current().id()
            || claim.session != self.run.session
            || claim.commands != 1
            || !claim.loans.is_empty()
            || claim.command_origin.is_some()
            || state.runs.contains_key(&original.run_unit_id)
        {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
}

fn require_frame(run: &Run, original: &Invocation, now_tick: u64) -> Result<(), HostProblem> {
    if run.program_abend.is_some() {
        return Err(HostProblem::UnknownOutcome);
    }
    if original.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if now_tick >= original.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    if run.current_program.effect_invocation != *original
        || run.current_program.logical_level != 1
        || original.parent_execution_id.is_some()
        || run.current_program.parent_execution_id.is_some()
        || super::super::task_context::current_program(original).as_ref()
            != run.current_program.current.as_ref()
        || run.current_program.current.is_none()
        || original
            .bindings
            .contains_key(CICS_NESTED_EFFECT_ORIGIN_BINDING)
        || original
            .bindings
            .contains_key(CICS_OUTER_EFFECT_ORIGIN_BINDING)
        || original
            .bindings
            .get("cics.execution-context")
            .is_none_or(|context| {
                context.schema() != "mainframe-env.cics.execution-context@1"
                    || context.bytes() != b"local"
            })
        || original.bindings.get("cics.session").is_none_or(|session| {
            session.schema() != "mainframe-env.cics.session@1"
                || session.bytes() != run.session.as_bytes()
        })
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok(())
}

struct ExitFence<'a> {
    service: &'a CicsService,
    session: String,
    armed: bool,
}

impl ExitFence<'_> {
    fn retain(&self) -> Result<(), HostProblem> {
        let mut state = self.service.lock()?;
        state
            .task_dispatch
            .uncertain_sessions
            .insert(self.session.clone());
        Ok(())
    }
}

impl Drop for ExitFence<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.retain();
        }
    }
}

impl CicsService {
    /// Borrow an already registered and synchronized current root program.
    ///
    /// This opt-in port never registers, restores, synchronizes, replaces or
    /// enrolls a task from the supplied invocation. The existing CommandLease
    /// owns the real Run outside the mutex throughout the callback and performs
    /// its original restoration. All ordinary CICS command paths are unchanged.
    ///
    /// Only a same-thread root frame with explicit local context/session is
    /// supported. Nested LINK, unknown, expired, cancelled or changed actors
    /// refuse. Callback panic, UnknownOutcome or failed restoration fences the
    /// actual session uncertain; there is no implicit MQ/UOW disposition.
    pub fn with_current_program_loan<T, F>(
        &self,
        original: &Invocation,
        now_tick: u64,
        body: F,
    ) -> Result<T, HostProblem>
    where
        F: for<'a> FnOnce(&CicsCurrentProgramLoan<'a>) -> Result<T, HostProblem>,
    {
        {
            let state = self.lock()?;
            let run = state
                .runs
                .get(&original.run_unit_id)
                .ok_or(HostProblem::Unauthorized)?;
            require_frame(run, original, now_tick)?;
            command_ready(&state, &original.run_unit_id)?;
            if state.task_dispatch.active(&original.run_unit_id) {
                return Err(HostProblem::Unauthorized);
            }
        }
        let mut command = CommandLease::acquire_with_origin(self, &original.run_unit_id, None)?;
        // Recheck the actual held Run after the acquire lock gap. No matching
        // identifiers or copied observation can replace the selected actor.
        require_frame(&command, original, now_tick)?;
        let mut fence = ExitFence {
            service: self,
            session: command.session.clone(),
            armed: true,
        };
        let result = {
            let loan = CicsCurrentProgramLoan {
                service: self,
                run: &command,
                _thread_confined: PhantomData,
            };
            loan.require_current(original, now_tick)?;
            body(&loan)
        };
        let unknown = matches!(&result, Err(HostProblem::UnknownOutcome));
        if unknown {
            // Retain uncertainty before restoring the real Run/releasing its
            // exclusive claim. No competing entry observes a known task here.
            fence.retain()?;
        }
        #[cfg(test)]
        if let Some(observe) = BEFORE_RESTORE.with(|slot| slot.borrow_mut().take()) {
            observe(self, original);
        }
        // Keep the owning command while restoring. If restoration fails, fence
        // before its original Drop can retry restoration/release.
        if command.restore().is_err() {
            let _ = fence.retain();
            return Err(HostProblem::UnknownOutcome);
        }
        if !unknown {
            fence.armed = false;
        }
        result
    }
}
