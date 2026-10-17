//! Opt-in compiled initial CICS first-dispatch boundary; no MQ settlement.
use super::*;
use mainframe_env_cics::{CicsCurrentProgramLoan, CicsService};
use mainframe_env_host_api::ProgramLinkSelection;
use mainframe_env_interpreter::{
    DurableEffectDispatch, MqMqiProgramFrame, ProtectedStorageAdmission,
    ProtectedStorageConfiguration, ProtectedStorageDispatchHooks,
};
use mainframe_env_store_api::{ArtifactRecord, ProviderStateIdentity};
use std::sync::Mutex;
#[cfg(test)]
mod tests;

/// Actual binary used to construct this route's machine, not execution authority.
/// No public constructor, Clone, Debug or serialization implementation.
pub struct CicsProtectedCompiledProgram<'a> {
    router: &'a DefaultProgramRouter,
    original: &'a Invocation,
    name: &'a str,
    catalog: &'a ProviderStateRecord,
    record: &'a ArtifactRecord,
}
impl CicsProtectedCompiledProgram<'_> {
    pub fn original(&self) -> &Invocation {
        self.original
    }
    pub fn program(&self) -> &str {
        self.name
    }
    pub fn artifact(&self) -> &ArtifactRef {
        &self.record.artifact
    }
    pub fn metadata(&self) -> &mainframe_env_store_api::ExecutableArtifactMetadata {
        self.record
            .executable
            .as_ref()
            .expect("admitted executable metadata")
    }
    pub fn content_digest(&self) -> &[u8; 32] {
        &self.record.payload_digest
    }
    fn require_current(&self) -> Result<(), HostProblem> {
        let store = self
            .router
            .cobol
            .store
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .router
            .cobol
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if store
            .get_provider_state(&self.catalog.namespace, &self.catalog.key)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .as_ref()
            != Some(self.catalog)
            || artifacts
                .get_artifact(&self.record.artifact)
                .map_err(|_| HostProblem::UnknownOutcome)?
                .as_ref()
                != Some(self.record)
            || self.original.artifact != self.record.artifact
            || self.original.selector.as_str() != format!("program:{}", self.name)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

/// All independently produced live loans at one actual compiled dispatch.
/// Construction is private; the callback cannot retain or serialize this proof.
pub struct CicsProtectedCompiledDispatch<'a> {
    program: &'a CicsProtectedCompiledProgram<'a>,
    storage: &'a ProtectedStorageAdmission<'a>,
    dispatch: &'a DurableEffectDispatch<'a>,
    task: &'a CicsCurrentProgramLoan<'a>,
    selection: &'a ProgramLinkSelection,
}
impl CicsProtectedCompiledDispatch<'_> {
    pub fn program(&self) -> &CicsProtectedCompiledProgram<'_> {
        self.program
    }
    pub fn storage(&self) -> &ProtectedStorageAdmission<'_> {
        self.storage
    }
    pub fn dispatch(&self) -> &DurableEffectDispatch<'_> {
        self.dispatch
    }
    pub fn task(&self) -> &CicsCurrentProgramLoan<'_> {
        self.task
    }
    pub fn selection(&self) -> &ProgramLinkSelection {
        self.selection
    }
    pub fn require_current(&self) -> Result<(), HostProblem> {
        let original = self.program.original;
        self.program.require_current()?;
        self.storage.require_current(original)?;
        self.dispatch.require_current(original)?;
        self.task
            .require_current(original, self.dispatch.control().now_tick)?;
        let artifacts = self
            .program
            .router
            .cobol
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.task
            .require_same_installed_storage(self.storage.store(), artifacts)?;
        if !Arc::ptr_eq(self.storage.store(), self.dispatch.store())
            || self.task.installed_program_selection()? != *self.selection
            || self.selection.artifact != original.artifact
            || matches!(self.dispatch.effect().request, HostRequest::Cics(_))
        {
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
}

/// Deliberate installed embedding; ordinary ProductServer setup has no route.
/// Matching context cannot supply a genuine MQ provider/manager/frame. Default
/// Unsupported dispatches nothing; no replacement result or settlement is accepted.
pub trait CicsProtectedCompiledHost {
    fn configuration(
        &mut self,
        _program: &CicsProtectedCompiledProgram<'_>,
    ) -> Result<ProtectedStorageConfiguration, HostProblem> {
        Err(HostProblem::Unsupported)
    }
    fn admit_initial_frame(
        &mut self,
        _program: &CicsProtectedCompiledProgram<'_>,
        _storage: &ProtectedStorageAdmission<'_>,
        _task: &CicsCurrentProgramLoan<'_>,
        _selection: &ProgramLinkSelection,
    ) -> Result<Option<Arc<dyn MqMqiProgramFrame>>, HostProblem> {
        Err(HostProblem::Unsupported)
    }
    fn with_dispatch(
        &mut self,
        _actual: &CicsProtectedCompiledDispatch<'_>,
        _invoke: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }
    fn retain_uncertain(&mut self, _original: &Invocation) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }
}

type FrameSlot = Arc<Mutex<Option<Option<Arc<dyn MqMqiProgramFrame>>>>>;
struct Hooks<'a> {
    program: CicsProtectedCompiledProgram<'a>,
    cics: &'a CicsService,
    terminal: &'a Invocation,
    host: &'a mut dyn CicsProtectedCompiledHost,
    frame: FrameSlot,
    selection: Option<ProgramLinkSelection>,
}
impl ProtectedStorageDispatchHooks for Hooks<'_> {
    fn admitted(&mut self, storage: &ProtectedStorageAdmission<'_>) -> Result<(), HostProblem> {
        self.program.require_current()?;
        storage.require_current(self.program.original)?;
        let store = self
            .program
            .router
            .cobol
            .store
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .program
            .router
            .cobol
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if !Arc::ptr_eq(storage.store(), store) {
            return Err(HostProblem::Unauthorized);
        }
        let mut frame = None;
        let mut frozen = None;
        self.cics.with_initial_terminal_program_loan(
            self.terminal,
            self.program.original,
            storage.claim().admission().event.tick,
            |task, selection| {
                task.require_same_installed_storage(store, artifacts)?;
                if selection.artifact != *self.program.artifact() {
                    return Err(HostProblem::Unauthorized);
                }
                self.program.require_current()?;
                frame =
                    Some(
                        self.host
                            .admit_initial_frame(&self.program, storage, task, selection)?,
                    );
                frozen = Some(selection.clone());
                Ok(())
            },
        )?;
        *self.frame.lock().map_err(|_| HostProblem::UnknownOutcome)? = frame;
        self.selection = frozen;
        Ok(())
    }
    fn with_dispatch(
        &mut self,
        storage: &ProtectedStorageAdmission<'_>,
        dispatch: &DurableEffectDispatch<'_>,
        invoke: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        if matches!(dispatch.effect().request, HostRequest::Cics(_)) {
            return Err(HostProblem::Unsupported);
        }
        let selection = self.selection.as_ref().ok_or(HostProblem::UnknownOutcome)?;
        self.cics.with_current_program_loan(
            dispatch.original(),
            dispatch.control().now_tick,
            |task| {
                let actual = CicsProtectedCompiledDispatch {
                    program: &self.program,
                    storage,
                    dispatch,
                    task,
                    selection,
                };
                // Every refusal after entering this first-dispatch loan is retained
                // while the original command claim still holds the real Run.
                if actual.require_current().is_ok() {
                    let _ = self.host.with_dispatch(&actual, invoke);
                }
                // Only the first dispatch is admitted. Fence CICS before lease release;
                // never feed a guessed successful CALL/RETURN into another drive.
                Err(HostProblem::UnknownOutcome)
            },
        )
    }
    fn retain_uncertain(
        &mut self,
        original: &Invocation,
        _storage: Option<&ProtectedStorageAdmission<'_>>,
    ) -> Result<(), HostProblem> {
        self.frame
            .lock()
            .map_err(|_| HostProblem::UnknownOutcome)?
            .take();
        self.host.retain_uncertain(original)
    }
}
impl DefaultProgramRouter {
    /// Construct the real selected binary and lend its first external dispatch
    /// under actual CICS task/storage/Intent custody. Always retain UnknownOutcome.
    /// The existing MQ profile validator still refuses unsupported CICS profiles;
    /// no fallback batch owner, implicit handle or structure version is supplied.
    pub fn execute_first_protected_cics_dispatch(
        &self,
        cics: &CicsService,
        terminal: &Invocation,
        actor: &Invocation,
        host: &mut dyn CicsProtectedCompiledHost,
    ) -> Result<ExecutionOutcome, HostProblem> {
        let store = self
            .cobol
            .store
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .cobol
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let scoped = self
            .cobol
            .host
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let control = self
            .cobol
            .control
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let name = actor
            .selector
            .as_str()
            .strip_prefix("program:")
            .ok_or(HostProblem::Unauthorized)?;
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            || actor.parent_execution_id.is_some()
        {
            return Err(HostProblem::Unauthorized);
        }
        let catalog = store
            .get_provider_state("online-program", name)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or(HostProblem::NotFound)?;
        if catalog.namespace != "online-program"
            || catalog.key != name
            || catalog.version != 1
            || catalog.payload != actor.artifact.as_str().as_bytes()
        {
            return Err(HostProblem::Unauthorized);
        }
        let record = artifacts
            .get_artifact(&actor.artifact)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or(HostProblem::NotFound)?;
        let executable = artifact::admit_executable_artifact(&record)?;
        let program = CicsProtectedCompiledProgram {
            router: self,
            original: actor,
            name,
            catalog: &catalog,
            record: &record,
        };
        program.require_current()?;
        let mut configuration = host.configuration(&program)?;
        let identity = ProviderStateIdentity {
            namespace: catalog.namespace.clone(),
            key: catalog.key.clone(),
        };
        if !configuration.provider_rows.contains(&identity) {
            if configuration.provider_rows.len() >= 16 {
                return Err(HostProblem::ResourceExhausted);
            }
            configuration.provider_rows.push(identity);
        }
        let mut machine = ReferenceMachine::from_binary(
            executable.payload(),
            actor.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        let frame: FrameSlot = Arc::new(Mutex::new(None));
        let mut hooks = Hooks {
            program,
            cics,
            terminal,
            host,
            frame: frame.clone(),
            selection: None,
        };
        let coordinator =
            ExecutionCoordinator::durable(scoped.clone(), store.clone(), Default::default());
        Ok(coordinator.execute_protected_storage_with_preparation(
            &mut machine,
            actor,
            configuration,
            &mut hooks,
            |machine| {
                let frame = frame
                    .lock()
                    .map_err(|_| HostProblem::UnknownOutcome)?
                    .take()
                    .ok_or(HostProblem::UnknownOutcome)?;
                if let Some(frame) = frame {
                    machine
                        .bind_mqi_program_frame(frame)
                        .map_err(|_| HostProblem::UnknownOutcome)?;
                }
                Ok(())
            },
            || control.observe(actor),
        ))
    }
}
