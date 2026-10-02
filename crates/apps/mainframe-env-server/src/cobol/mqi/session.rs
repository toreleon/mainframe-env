//! One bounded frame lifetime. Drop invalidates transport only.

use super::*;
use mainframe_env_host_api::{MqHconn, mq_mqi::MqMqiUnitOfWork};
use mainframe_env_interpreter::MqMqiProgramProfile;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::AtomicBool;

/// Trusted embedding session. No method implies task/process termination.
/// Preparation abort retires only newly owned volatile admission; finish observes
/// the untouched raw coordinator outcome. Neither may erase protected CALL/core
/// state. Drop must not commit/backout/disconnect or schedule detached cleanup.
pub trait InstalledMqFrameSession: Send {
    /// Physical adapter and control actually bound to this trusted session.
    /// Matching them is necessary but does not attest service/root lifecycle.
    fn store(&self) -> &Arc<dyn PlatformStore>;
    fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl>;
    fn program_frame(&self) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem>;
    fn abort_preparation(&mut self, problem: &HostProblem) -> Result<(), HostProblem>;
    fn finish(&mut self, outcome: &ExecutionOutcome) -> Result<(), HostProblem>;
}

struct ExecutableFrame {
    active: Arc<AtomicBool>,
    invocation: Invocation,
    inner: Arc<dyn MqMqiProgramFrame>,
}
impl ExecutableFrame {
    fn observe<T>(
        &self,
        invocation: &Invocation,
        callback: impl FnOnce(&dyn MqMqiProgramFrame) -> Result<T, HostProblem>,
    ) -> Result<T, HostProblem> {
        if !self.active.load(Ordering::Acquire) || invocation != &self.invocation {
            return Err(HostProblem::Unauthorized);
        }
        let result = match catch_unwind(AssertUnwindSafe(|| callback(self.inner.as_ref()))) {
            Ok(result) => result,
            Err(_) => {
                // An uncertain callback cannot become a usable observation on retry.
                self.active.store(false, Ordering::Release);
                return Err(HostProblem::UnknownOutcome);
            }
        };
        // A callback cannot keep a usable transport after synchronous invalidation.
        if !self.active.load(Ordering::Acquire) {
            return Err(HostProblem::Unauthorized);
        }
        result
    }
}
impl MqMqiProgramFrame for ExecutableFrame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.observe(invocation, |frame| frame.profile(invocation))
    }

    fn local_unit(
        &self,
        invocation: &Invocation,
        connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        self.observe(invocation, |frame| frame.local_unit(invocation, connection))
    }
}

pub(in crate::cobol) struct SessionGuard {
    session: Option<Box<dyn InstalledMqFrameSession>>,
    active: Arc<AtomicBool>,
    pub(in crate::cobol) observed_tick: u64,
    pub(in crate::cobol) original_core: Option<mainframe_env_store_api::EffectRecord>,
}
impl SessionGuard {
    pub(in crate::cobol) fn new(session: Box<dyn InstalledMqFrameSession>) -> Self {
        Self {
            session: Some(session),
            active: Arc::new(AtomicBool::new(true)),
            observed_tick: 0,
            original_core: None,
        }
    }
    pub(in crate::cobol) fn frame(
        &self,
        invocation: &Invocation,
    ) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        let session = self.session.as_ref().ok_or(HostProblem::UnknownOutcome)?;
        let inner = catch_unwind(AssertUnwindSafe(|| session.program_frame()))
            .map_err(|_| HostProblem::UnknownOutcome)??;
        Ok(Arc::new(ExecutableFrame {
            active: self.active.clone(),
            invocation: invocation.clone(),
            inner,
        }))
    }
    pub(super) fn matches_setup(
        &self,
        store: &Arc<dyn PlatformStore>,
        control: &Arc<dyn ProgramExecutionControl>,
    ) -> Result<(), HostProblem> {
        let session = self.session.as_ref().ok_or(HostProblem::UnknownOutcome)?;
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            if Arc::ptr_eq(session.store(), store)
                && Arc::ptr_eq(session.execution_control(), control)
            {
                Ok(())
            } else {
                Err(HostProblem::Unauthorized)
            }
        }))
        .unwrap_or(Err(HostProblem::UnknownOutcome))
    }
    pub(in crate::cobol) fn abort(&mut self, problem: HostProblem) -> HostProblem {
        self.active.store(false, Ordering::Release);
        let Some(mut session) = self.session.take() else {
            return HostProblem::UnknownOutcome;
        };
        match catch_unwind(AssertUnwindSafe(|| session.abort_preparation(&problem))) {
            Ok(Ok(())) => problem,
            _ => HostProblem::UnknownOutcome,
        }
    }
    pub(in crate::cobol) fn finish(
        &mut self,
        outcome: &ExecutionOutcome,
    ) -> Result<(), HostProblem> {
        self.active.store(false, Ordering::Release);
        let mut session = self.session.take().ok_or(HostProblem::UnknownOutcome)?;
        match catch_unwind(AssertUnwindSafe(|| session.finish(outcome))) {
            Ok(Ok(())) => Ok(()),
            _ => Err(HostProblem::UnknownOutcome),
        }
    }
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        // No embedding cleanup callback on unwind/abandonment. Durable uncertainty
        // remains with the original core/CALL protocol, never a cleanup queue.
    }
}
