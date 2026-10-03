use super::*;
use mainframe_env_execution_api::ExecutionOutcome;
use mainframe_env_host_api::mq_mqi::MqMqiUnitOfWork;
use mainframe_env_host_api::mq_raw_layout::{
    MqConnxProfile, MqRawCharacterEncoding, MqRawNumberEncoding, MqRawStructureEncoding,
};
use mainframe_env_host_api::{MqHconn, MqMqiEffectOccurrence};
use mainframe_env_interpreter::{MqMqiConnxProfile, MqMqiProgramFrame, MqMqiProgramProfile};
use mainframe_env_mq::MqTrustedBatchFrame;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
pub(super) struct State {
    pub(super) facet: MqTrustedBatchFrame,
    dispatched: bool,
    floor: u64,
}
pub(super) struct ClosedFrame {
    original: Invocation,
    root_execution: ExecutionId,
    control: Arc<dyn ProgramExecutionControl>,
    active: AtomicBool,
    state: Mutex<State>,
    compiled: Option<CompiledFrame>,
}
/// Frozen genuine published/catalog admission, retained after normal child return.
/// This read-only observation grants no executable/lifecycle authority.
pub(super) struct CompiledFrame {
    pub(super) catalog: mainframe_env_store_api::ProviderStateRecord,
    pub(super) metadata: mainframe_env_store_api::ExecutableArtifactMetadata,
    pub(super) content: [u8; 32],
}
impl ClosedFrame {
    pub(super) fn compiled(&self) -> Option<&CompiledFrame> {
        self.compiled.as_ref()
    }
    pub(super) fn new_compiled(
        facet: MqTrustedBatchFrame,
        control: Arc<dyn ProgramExecutionControl>,
        floor: u64,
        compiled: Option<CompiledFrame>,
    ) -> Self {
        let mut value = Self::new(facet, control, floor);
        value.compiled = compiled;
        value
    }
    pub(super) fn native_root_execution(&self) -> ExecutionId {
        // Frozen from the retained opaque facet; no mutex or poison fallback
        // supplies an invented root identity while a topology map is held.
        self.root_execution.clone()
    }
    pub(super) fn new(
        facet: MqTrustedBatchFrame,
        control: Arc<dyn ProgramExecutionControl>,
        floor: u64,
    ) -> Self {
        Self {
            original: facet.original().clone(),
            root_execution: facet.root_execution().clone(),
            control,
            active: AtomicBool::new(true),
            compiled: None,
            state: Mutex::new(State {
                facet,
                dispatched: false,
                floor,
            }),
        }
    }
    pub(super) fn check_original(&self, original: &Invocation) -> Result<(), HostProblem> {
        if !self.active.load(Ordering::SeqCst) || &self.original != original {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
    fn live(&self, state: &mut State) -> Result<(), HostProblem> {
        self.check_original(&self.original)?;
        self.controls(state)?;
        self.check_original(&self.original)
    }
    fn controls(&self, state: &mut State) -> Result<(), HostProblem> {
        let control = catch_unwind(AssertUnwindSafe(|| self.control.observe(&self.original)))
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if control.now_tick < state.floor || control.now_tick == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        state.floor = control.now_tick;
        if control.cancellation_requested || self.original.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        if self.original.deadline_tick == u64::MAX
            || control.now_tick >= self.original.deadline_tick
        {
            return Err(HostProblem::TimedOut);
        }
        Ok(())
    }
    pub(super) fn with_active<T>(
        &self,
        original: &Invocation,
        callback: impl FnOnce(&mut State) -> Result<T, HostProblem>,
    ) -> Result<T, HostProblem> {
        self.check_original(original)?;
        let mut state = self.state.lock().map_err(|_| HostProblem::UnknownOutcome)?;
        self.live(&mut state)?;
        match catch_unwind(AssertUnwindSafe(|| callback(&mut state))) {
            Ok(result) => {
                self.live(&mut state)?;
                result
            }
            Err(_) => {
                self.uncertain(&mut state);
                Err(HostProblem::UnknownOutcome)
            }
        }
    }
    fn uncertain(&self, state: &mut State) {
        self.active.store(false, Ordering::SeqCst);
        let _ = catch_unwind(AssertUnwindSafe(|| state.facet.retain_uncertain()));
    }
    pub(super) fn dispatch(
        &self,
        original: &Invocation,
        effect: MqMqiEffectOccurrence<'_>,
    ) -> Result<EffectResult, HostProblem> {
        self.check_original(original)?;
        let mut state = self.state.lock().map_err(|_| HostProblem::UnknownOutcome)?;
        self.live(&mut state)?;
        state.dispatched = true;
        let result = catch_unwind(AssertUnwindSafe(|| state.facet.dispatch(effect)));
        if self.live(&mut state).is_err() {
            self.uncertain(&mut state);
            return Err(HostProblem::UnknownOutcome);
        }
        match result {
            Ok(Ok(reply)) if !matches!(reply.outcome, Err(HostProblem::UnknownOutcome)) => {
                Ok(reply)
            }
            Ok(Err(problem)) if problem != HostProblem::UnknownOutcome => Err(problem),
            _ => {
                self.uncertain(&mut state);
                Err(HostProblem::UnknownOutcome)
            }
        }
    }
    pub(super) fn abort(&self) -> Result<(), HostProblem> {
        if !self.active.swap(false, Ordering::SeqCst) {
            return Err(HostProblem::Unauthorized);
        }
        let mut state = self.state.lock().map_err(|_| HostProblem::UnknownOutcome)?;
        if state.dispatched {
            self.uncertain(&mut state);
            return Err(HostProblem::UnknownOutcome);
        }
        let result = state.facet.abort_preparation();
        self.active.store(false, Ordering::SeqCst);
        if result.is_err() {
            self.uncertain(&mut state);
        }
        result
    }
    fn finish(&self, outcome: &ExecutionOutcome) -> Result<(), HostProblem> {
        // Revoke before waiting for any in-flight observation/dispatch. Its
        // postcheck converts a racing publication into protected uncertainty.
        if !self.active.swap(false, Ordering::SeqCst) {
            return Err(HostProblem::Unauthorized);
        }
        let mut state = self.state.lock().map_err(|_| HostProblem::UnknownOutcome)?;
        if !matches!(outcome, ExecutionOutcome::Completed(_)) || self.controls(&mut state).is_err()
        {
            self.uncertain(&mut state);
            return Err(HostProblem::UnknownOutcome);
        }
        let result = state.facet.return_normal();
        self.active.store(false, Ordering::SeqCst);
        if result.is_err() {
            self.uncertain(&mut state);
        }
        result
    }
    #[cfg(test)]
    pub(super) fn test_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}
impl CompiledFrame {
    pub(super) fn charge(
        proof: &InstalledBatchAdmission<'_>,
        ceiling: usize,
    ) -> Result<usize, HostProblem> {
        let catalog = proof.catalog_record().ok_or(HostProblem::Unsupported)?;
        let metadata = proof.artifact_metadata();
        if !metadata.validate()
            || catalog.payload.len() > 128
            || catalog.namespace != "batch-program"
            || catalog.payload != proof.child().artifact.as_str().as_bytes()
        {
            return Err(HostProblem::Unauthorized);
        }
        let mut bytes = 4096_usize;
        for (key, value) in &metadata.options {
            bytes = bytes
                .checked_add(key.len() + value.len() + 128)
                .ok_or(HostProblem::ResourceExhausted)?;
        }
        for value in metadata
            .host_interfaces
            .iter()
            .chain(metadata.dialect_contracts.iter().flatten())
        {
            bytes = bytes
                .checked_add(value.len() + 128)
                .ok_or(HostProblem::ResourceExhausted)?;
        }
        if bytes > ceiling {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(bytes)
    }
    pub(super) fn from_checked(proof: &InstalledBatchAdmission<'_>) -> Option<Self> {
        Some(Self {
            catalog: proof.catalog_record()?.clone(),
            metadata: proof.artifact_metadata().clone(),
            content: proof.content_digest(),
        })
    }
}
pub(super) struct Observation(pub(super) Arc<ClosedFrame>);
impl MqMqiProgramFrame for Observation {
    fn profile(&self, original: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        self.0.with_active(original, |state| {
            Ok(MqMqiProgramProfile {
                context: state.facet.context()?,
                limits: state.facet.limits(),
            })
        })
    }
    fn local_unit(
        &self,
        original: &Invocation,
        connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        self.0
            .with_active(original, |state| state.facet.current_unit(connection))
    }
    fn connx_profile(&self, original: &Invocation) -> Result<MqMqiConnxProfile, HostProblem> {
        self.profile(original)?;
        Ok(MqMqiConnxProfile {
            profile: MqConnxProfile::OrdinaryOwnedNonshared,
            encoding: MqRawStructureEncoding {
                numbers: MqRawNumberEncoding::NormalBigEndian,
                characters: MqRawCharacterEncoding::AsciiCompatible,
            },
        })
    }
}
pub(super) struct Session {
    frame: Arc<ClosedFrame>,
    store: Arc<dyn PlatformStore>,
    control: Arc<dyn ProgramExecutionControl>,
    finished: bool,
}
impl Session {
    pub(super) fn new(
        frame: Arc<ClosedFrame>,
        store: Arc<dyn PlatformStore>,
        control: Arc<dyn ProgramExecutionControl>,
    ) -> Self {
        Self {
            frame,
            store,
            control,
            finished: false,
        }
    }
}
impl InstalledMqFrameSession for Session {
    fn store(&self) -> &Arc<dyn PlatformStore> {
        &self.store
    }
    fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl> {
        &self.control
    }
    fn program_frame(&self) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        self.frame.check_original(&self.frame.original)?;
        Ok(Arc::new(Observation(self.frame.clone())))
    }
    fn abort_preparation(&mut self, _: &HostProblem) -> Result<(), HostProblem> {
        if self.finished {
            return Err(HostProblem::Unauthorized);
        }
        self.finished = true;
        self.frame.abort()
    }
    fn finish(&mut self, outcome: &ExecutionOutcome) -> Result<(), HostProblem> {
        if self.finished {
            return Err(HostProblem::Unauthorized);
        }
        self.finished = true;
        self.frame.finish(outcome)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // Transport revocation only. No service callback, frame retirement,
        // Drop UOW decision, detached work or invented task-end disposition.
        self.frame.active.store(false, Ordering::SeqCst);
    }
}
