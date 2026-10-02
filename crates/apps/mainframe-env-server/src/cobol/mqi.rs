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
        if self.cobol.host.get().is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.cobol
            .mqi_host
            .set(host)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }
}

impl CobolProgram {
    pub(super) fn admit_batch_mqi(
        &self,
        invocation: &mut Invocation,
    ) -> Result<Option<Arc<dyn MqMqiProgramFrame>>, HostProblem> {
        let Some(host) = self.mqi_host.get() else {
            return Ok(None);
        };
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
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        host.admit_installed_batch(invocation, store.as_ref())
            .map(Some)
    }
}

#[cfg(test)]
mod tests;
