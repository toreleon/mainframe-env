//! Trusted installed-batch admission seam. No selected provider is registered.

use super::*;
use mainframe_env_interpreter::MqMqiProgramFrame;
mod proof;
mod session;
pub use proof::InstalledBatchAdmission;
pub use session::InstalledMqFrameSession;
pub(super) use session::SessionGuard;

/// Configured at product setup, before runtime binding. The embedding must use
/// the selected MQ service's same store and independently admitted lifecycle;
/// a decoded invocation binding or application owner assertion is not a frame.
/// Calls happen only after installed artifact admission and construction of the
/// actual program Invocation. No application request chooses this factory.
pub trait ProgramMqHostAdmission: Send + Sync {
    fn admit_installed_batch(
        &self,
        admission: &InstalledBatchAdmission<'_>,
    ) -> Result<Box<dyn InstalledMqFrameSession>, HostProblem>;
}

impl DefaultProgramRouter {
    pub fn bind_mqi_program_host(
        &self,
        host: Arc<dyn ProgramMqHostAdmission>,
    ) -> Result<(), HostProblem> {
        let setup = self
            .cobol
            .setup
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.cobol.bind_mqi_host_locked(&setup, host)
    }
}

impl CobolProgram {
    fn bind_mqi_host_locked(
        &self,
        _setup: &std::sync::MutexGuard<'_, ()>,
        host: Arc<dyn ProgramMqHostAdmission>,
    ) -> Result<(), HostProblem> {
        if self.host.get().is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.mqi_host
            .set(host)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }

    pub(super) fn bind_runtime_locked(
        &self,
        _setup: &std::sync::MutexGuard<'_, ()>,
        host: Arc<ScopedHostService>,
        store: Arc<dyn PlatformStore>,
        artifacts: Arc<dyn ArtifactStore>,
    ) -> Result<(), HostProblem> {
        // All production setters share setup. Reject partial/repeated setup before publishing.
        if self.host.get().is_some() || self.store.get().is_some() || self.artifacts.get().is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.host
            .set(host)
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        self.store
            .set(store)
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        self.artifacts
            .set(artifacts)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }

    pub(super) fn validate_typed_parent(
        &self,
        parent: &Invocation,
        effect: &EffectRequest,
    ) -> Result<(), HostProblem> {
        if self.mqi_host.get().is_none() {
            return Ok(());
        }
        let now = self
            .observe_execution_control(parent)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        check_controls(parent, now, 0)?;
        if self.control.get().is_none() {
            return Err(HostProblem::InfrastructureFailure);
        }
        proof::observe_parent(self, parent, effect, now.now_tick)?;
        Ok(())
    }

    pub(super) fn admit_batch_mqi(
        &self,
        invocation: &mut Invocation,
        admitted: Option<&AdmittedProgram>,
        call: Option<&replay::WinningInstalledCall<'_>>,
    ) -> Result<Option<SessionGuard>, HostProblem> {
        let (host, store, control) = {
            let _setup = self
                .setup
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let Some(host) = self.mqi_host.get() else {
                return Ok(None);
            };
            if call.is_none() || admitted.is_none() {
                return Err(HostProblem::Unsupported);
            }
            if self.host.get().is_none() || self.artifacts.get().is_none() {
                return Err(HostProblem::InfrastructureFailure);
            }
            let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
            // A typed producer cannot silently select a different clock domain.
            let control = self
                .control
                .get()
                .ok_or(HostProblem::InfrastructureFailure)?;
            (Arc::clone(host), Arc::clone(store), Arc::clone(control))
        };
        // Invoke embedding code only after releasing setup; it may call back into the router.
        // This first frame profile does not admit nested CICS/task or IMS
        // ownership. Do not erase their provenance to impersonate ordinary batch.
        if invocation.bindings.contains_key("cics.execution-context")
            || invocation
                .bindings
                .contains_key("cics.nested-effect-origin")
            || invocation.bindings.contains_key("cics.outer-effect-origin")
        {
            return Err(HostProblem::Unsupported);
        }
        let call = call.ok_or(HostProblem::Unsupported)?;
        let admitted = admitted.ok_or(HostProblem::Unsupported)?;
        let before = self
            .observe_execution_control(call.parent())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        check_controls(call.parent(), before, 0)?;
        if let Some(binding) = invocation.bindings.get("mq.host-context") {
            // This configured factory admits only ordinary local-MQ batch. An
            // inherited context is provenance, never a value to normalize away.
            if binding.schema() != "mainframe-env.mq.host-context@1"
                || binding.bytes() != b"zos-batch|queue-manager"
            {
                return Err(HostProblem::Malformed);
            }
        } else {
            if invocation.bindings.len() >= InvocationLimits::default().max_bindings {
                return Err(HostProblem::ResourceExhausted);
            }
            invocation.bindings.insert(
                "mq.host-context".into(),
                BoundedPayload::new(
                    "mainframe-env.mq.host-context@1",
                    b"zos-batch|queue-manager".to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
        let admission = proof::admitted(
            self,
            call.parent(),
            invocation,
            admitted,
            call,
            &store,
            &control,
            before,
        )?;
        let session = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.admit_installed_batch(&admission)
        }))
        .map_err(|_| HostProblem::UnknownOutcome)??;
        let mut guard = SessionGuard::new(session);
        guard.original_core = Some(admission.core_intent().clone());
        // Callback may cancel, advance time or change retained admission state.
        let recheck = (|| {
            guard.matches_setup(&store, &control)?;
            let after = self
                .observe_execution_control(call.parent())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            check_controls(call.parent(), after, before.now_tick)?;
            guard.observed_tick = after.now_tick;
            let current = proof::admitted(
                self,
                call.parent(),
                invocation,
                admitted,
                call,
                &store,
                &control,
                after,
            )?;
            if current.core_intent() != admission.core_intent()
                || current.running_parent() != admission.running_parent()
            {
                return Err(HostProblem::UnknownOutcome);
            }
            Ok(())
        })();
        if let Err(problem) = recheck {
            return Err(guard.abort(problem));
        }
        Ok(Some(guard))
    }
}

pub(super) fn recheck_driving_parent(
    program: &CobolProgram,
    call: &replay::WinningInstalledCall<'_>,
    control: ExecutionControl,
    floor: &mut u64,
    expected_core: &mainframe_env_store_api::EffectRecord,
) -> Result<(), HostProblem> {
    if control.now_tick < *floor || control.now_tick == 0 || control.now_tick > i64::MAX as u64 {
        return Err(HostProblem::InfrastructureFailure);
    }
    *floor = control.now_tick;
    // Preserve the coordinator's own raw cancellation/deadline dispositions.
    if control.cancellation_requested
        || call.parent().cancellation_requested()
        || control.now_tick >= call.parent().deadline_tick
    {
        return Ok(());
    }
    let (core, _) = proof::observe_parent(program, call.parent(), call.effect(), control.now_tick)?;
    if &core != expected_core {
        return Err(HostProblem::UnknownOutcome);
    }
    call.recheck()
}

fn check_controls(
    invocation: &Invocation,
    control: ExecutionControl,
    floor: u64,
) -> Result<(), HostProblem> {
    if control.now_tick < floor
        || control.now_tick == 0
        || control.now_tick > i64::MAX as u64
        || invocation.deadline_tick > i64::MAX as u64
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    if control.cancellation_requested || invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if control.now_tick >= invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
