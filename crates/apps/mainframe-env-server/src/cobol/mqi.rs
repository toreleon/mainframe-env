//! Trusted installed-batch admission seam. No selected provider is registered.

use super::*;
use mainframe_env_interpreter::MqMqiProgramFrame;

/// Configured at product setup, before runtime binding. The embedding must use
/// the selected MQ service's same store and independently admitted lifecycle;
/// a decoded invocation binding or application owner assertion is not a frame.
/// Calls happen only after installed artifact admission and construction of the
/// actual program Invocation. No application request chooses this factory.
pub trait ProgramMqHostAdmission: Send + Sync {
    fn admit_installed_batch(
        &self,
        invocation: &Invocation,
        store: &dyn PlatformStore,
    ) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem>;
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

    pub(super) fn admit_batch_mqi(
        &self,
        invocation: &mut Invocation,
    ) -> Result<Option<Arc<dyn MqMqiProgramFrame>>, HostProblem> {
        let (host, store) = {
            let _setup = self
                .setup
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let Some(host) = self.mqi_host.get() else {
                return Ok(None);
            };
            let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
            (Arc::clone(host), Arc::clone(store))
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
        invocation.bindings.insert(
            "mq.host-context".into(),
            BoundedPayload::new(
                "mainframe-env.mq.host-context@1",
                b"zos-batch|queue-manager".to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        host.admit_installed_batch(invocation, store.as_ref())
            .map(Some)
    }
}

#[cfg(test)]
mod tests;
