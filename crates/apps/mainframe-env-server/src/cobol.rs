use mainframe_env_batch::{
    Program, ProgramInput, ProgramOutput, ProgramRouter, SystemServiceProgram,
    system_service_program,
};
use mainframe_env_cics::cics_abi_library;
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_db2::db2_abi_library;
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, ExecutionOutcome, IdempotencyKey, Invocation,
    InvocationLimits, Principal, RequestId, RunUnitId, Selector, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, ProgramRequest, RuntimeServiceKind, RuntimeServiceSelector, ScopedHostService,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionControlError, ExecutionCoordinator,
    ReferenceMachine, encode_cobol_call_result,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_mq::mq_abi_library;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits, materialize_host_abi_libraries,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactStore, PlatformStore, ProviderStateRecord, ProviderStateWrite,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// An embedding can supply a logical clock and a run-scoped cancellation source.
/// All nested invocations use the same source and inherited deadline/scope.
/// Default production ticks are Unix milliseconds advanced by a monotonic clock.
pub trait ProgramExecutionControl: Send + Sync {
    fn observe(&self, invocation: &Invocation) -> Result<ExecutionControl, ExecutionControlError>;
}

impl<F> ProgramExecutionControl for F
where
    F: Fn(&Invocation) -> Result<ExecutionControl, ExecutionControlError> + Send + Sync,
{
    fn observe(&self, invocation: &Invocation) -> Result<ExecutionControl, ExecutionControlError> {
        self(invocation)
    }
}

pub struct DefaultProgramRouter {
    router: Arc<ProgramRouter>,
    cobol: Arc<CobolProgram>,
}

impl DefaultProgramRouter {
    /// Bind at setup, before dispatch. Rebinding cannot change an active clock domain.
    pub fn bind_execution_control(
        &self,
        source: Arc<dyn ProgramExecutionControl>,
    ) -> Result<(), HostProblem> {
        self.cobol
            .control
            .set(source)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }

    pub(crate) fn observe_execution_control(
        &self,
        invocation: &Invocation,
    ) -> Result<ExecutionControl, ExecutionControlError> {
        self.cobol.observe_execution_control(invocation)
    }

    pub(crate) fn bind_runtime(
        &self,
        host: Arc<ScopedHostService>,
        store: Arc<dyn PlatformStore>,
        artifact_root: &Path,
    ) -> Result<(), HostProblem> {
        self.cobol
            .host
            .set(host)
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        self.cobol
            .store
            .set(store)
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        self.cobol
            .artifacts
            .set(
                LocalArtifactStore::open(artifact_root, 64 * 1024 * 1024)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            )
            .map_err(|_| HostProblem::IdempotencyConflict)
    }
}

impl HostProvider for DefaultProgramRouter {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.router.descriptor()
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        if let HostRequest::Program(ProgramRequest::Cancel { programs }) = &effect.request {
            return EffectResult {
                sequence: effect.sequence,
                outcome: self.cobol.cancel_instances(invocation, &effect, programs),
            };
        }
        if let HostRequest::Program(ProgramRequest::Call {
            payload,
            service: Some(service),
            ..
        }) = &effect.request
            && payload.schema() == "mainframe-env.cobol.call@1"
        {
            return EffectResult {
                sequence: effect.sequence,
                outcome: self
                    .cobol
                    .execute_runtime_service(service, payload)
                    .map(HostResult::Program),
            };
        }
        if let HostRequest::Program(ProgramRequest::Call {
            program,
            payload,
            service: None,
        }) = &effect.request
            && payload.schema() == "mainframe-env.cobol.call@1"
        {
            return EffectResult {
                sequence: effect.sequence,
                outcome: self
                    .cobol
                    .execute_installed_effect(invocation, &effect, program.as_str(), payload)
                    .map(HostResult::Program),
            };
        }
        if let HostRequest::Program(ProgramRequest::Call {
            program,
            payload,
            service: None,
        }) = &effect.request
            && payload.schema() == "mainframe-env.program.input@1"
            && !self
                .router
                .supported_programs()
                .any(|name| name.eq_ignore_ascii_case(program.as_str()))
        {
            return EffectResult {
                sequence: effect.sequence,
                outcome: self
                    .cobol
                    .execute_installed_effect(invocation, &effect, program.as_str(), payload)
                    .map(HostResult::Program),
            };
        }
        self.router.invoke(invocation, effect)
    }
}

#[must_use]
pub fn default_program_router() -> Arc<DefaultProgramRouter> {
    let cobol = Arc::new(CobolProgram::new());
    let router = ProgramRouter::with_builtins_and(
        BTreeMap::from([("COBOL".into(), cobol.clone() as Arc<dyn Program>)]),
        InvocationLimits::default(),
    )
    .expect("owned program catalog is valid");
    Arc::new(DefaultProgramRouter { router, cobol })
}

#[must_use]
pub const fn compatible_system_services() -> &'static [&'static str] {
    &["CEEDAYS", "COBDATFT", "MVSWAIT", "CEE3ABD"]
}

pub(crate) fn bind_compatible_runtime_services(
    invocation: &mut Invocation,
) -> Result<(), HostProblem> {
    let limits = InvocationLimits::default();
    for name in compatible_system_services() {
        let key = format!("cobol.runtime-service.{name}");
        let value = format!("le:{name}:1");
        if let Some(existing) = invocation.bindings.get(&key) {
            if existing.schema() != "mainframe-env.runtime-service-selector@1"
                || existing.bytes() != value.as_bytes()
            {
                return Err(HostProblem::Malformed);
            }
            continue;
        }
        if invocation.bindings.len() >= limits.max_bindings {
            return Err(HostProblem::ResourceExhausted);
        }
        invocation.bindings.insert(
            key,
            BoundedPayload::new(
                "mainframe-env.runtime-service-selector@1",
                value.into_bytes(),
                limits,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(())
}

fn with_compatible_runtime_services(mut invocation: Invocation) -> Result<Invocation, HostProblem> {
    bind_compatible_runtime_services(&mut invocation)?;
    Ok(invocation)
}

fn persist_batch_file_cursors(
    store: &dyn PlatformStore,
    key: &str,
    cursors: &BTreeMap<String, String>,
    current_version: Option<u64>,
) -> Result<(), HostProblem> {
    if cursors.is_empty() {
        if let Some(version) = current_version {
            store
                .delete_provider_state("batch-file-cursor", key, version)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
        }
        return Ok(());
    }
    let version = current_version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "batch-file-cursor".into(),
                key: key.into(),
                version,
                payload: serde_json::to_vec(cursors).map_err(|_| HostProblem::ProviderFailure)?,
            },
            current_version,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)
}

struct CobolProgram {
    host: OnceLock<Arc<ScopedHostService>>,
    store: OnceLock<Arc<dyn PlatformStore>>,
    artifacts: OnceLock<LocalArtifactStore>,
    sequence: AtomicU64,
    control: OnceLock<Arc<dyn ProgramExecutionControl>>,
    clock_start: Instant,
    clock_epoch: Option<u64>,
}

impl CobolProgram {
    fn new() -> Self {
        Self {
            host: OnceLock::new(),
            store: OnceLock::new(),
            artifacts: OnceLock::new(),
            sequence: AtomicU64::new(1),
            control: OnceLock::new(),
            clock_start: Instant::now(),
            clock_epoch: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|duration| u64::try_from(duration.as_millis()).ok()),
        }
    }

    fn observe_execution_control(
        &self,
        invocation: &Invocation,
    ) -> Result<ExecutionControl, ExecutionControlError> {
        let mut observation = if let Some(source) = self.control.get() {
            source.observe(invocation)?
        } else {
            let elapsed = u64::try_from(self.clock_start.elapsed().as_millis())
                .map_err(|_| ExecutionControlError::Unavailable)?;
            ExecutionControl {
                now_tick: self
                    .clock_epoch
                    .and_then(|epoch| epoch.checked_add(elapsed))
                    .ok_or(ExecutionControlError::Unavailable)?,
                cancellation_requested: false,
            }
        };
        observation.cancellation_requested |= invocation.cancellation.is_some();
        if let Some(binding) = invocation.bindings.get("jes.work-id") {
            if binding.schema() != "mainframe-env.jes-work@1" {
                return Err(ExecutionControlError::Unavailable);
            }
            let id = std::str::from_utf8(binding.bytes())
                .map_err(|_| ExecutionControlError::Unavailable)?;
            let store = self.store.get().ok_or(ExecutionControlError::Unavailable)?;
            let work = store
                .get_work(id)
                .map_err(|_| ExecutionControlError::Unavailable)?;
            // Offline batch execution need not own a scheduler row. If a row is
            // present, its cancellation state is authoritative, including nested CALLs.
            observation.cancellation_requested |= work.is_some_and(|work| {
                work.cancellation_requested
                    || work.state == mainframe_env_store_api::WorkState::Cancelled
            });
        }
        Ok(observation)
    }

    fn execute_installed(
        &self,
        parent: &Invocation,
        program: &str,
        payload: &BoundedPayload,
        identity: &str,
        writes: &mut Vec<ProviderStateWrite>,
    ) -> Result<BoundedPayload, HostProblem> {
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let name = program.to_ascii_uppercase();
        let catalog = match store
            .get_provider_state("batch-program", &name)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            Some(catalog) => catalog,
            None => store
                .get_provider_state("online-program", &name)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .ok_or_else(|| HostProblem::Condition {
                    name: format!("PROGRAM-NOTFOUND:{name}"),
                    response: -7,
                    response2: 0,
                })?,
        };
        let artifact = ArtifactRef::new(
            String::from_utf8(catalog.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = artifacts
            .get_artifact(&artifact)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or_else(|| HostProblem::Condition {
                name: format!("ARTIFACT-NOTFOUND:{name}"),
                response: -8,
                response2: 0,
            })?;
        let call_values = decode_cobol_call_values(payload)?;
        let limits = InvocationLimits::default();
        let sequence = identity;
        let mut bindings = parent.bindings.clone();
        bindings.insert("cobol.call.arguments".into(), payload.clone());
        let invocation = Invocation::new(
            RequestId::new(format!("online-call-request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("online-call-execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.run_unit_id.clone(),
            Some(parent.execution_id.clone()),
            Selector::new(format!("program:{}", program.to_ascii_uppercase()), limits)
                .map_err(|_| HostProblem::Malformed)?,
            artifact,
            Principal::new(
                parent.principal.id().clone(),
                parent.principal.grants().clone(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.service_class,
            parent.priority,
            parent.deadline_tick,
            TraceId::new(format!("online-call-trace-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("online-call-effect-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.attempt,
            parent.limits,
            bindings,
            limits,
        )
        .and_then(|invocation| {
            invocation.with_provider_generations(parent.provider_generations.clone(), limits)
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut invocation = with_compatible_runtime_services(invocation)?;
        invocation.cancellation = parent.cancellation.clone();
        let mut machine = ReferenceMachine::from_binary(
            &record.payload,
            invocation.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        let cursor_key = format!("{}:{name}", parent.run_unit_id.as_str());
        let cursor_record = store
            .get_provider_state("batch-file-cursor", &cursor_key)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let cursor_version = cursor_record.as_ref().map(|record| record.version);
        let loaded_cursors = cursor_record
            .map(|record| {
                serde_json::from_slice(&record.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            })
            .transpose()?
            .unwrap_or_default();
        machine
            .install_dataset_cursors(loaded_cursors)
            .map_err(|_| HostProblem::ResourceExhausted)?;
        let lease = instance::Lease::acquire(store.as_ref(), &invocation, &name, &mut machine)?;
        let coordinator = ExecutionCoordinator::durable(
            Arc::clone(self.host.get().ok_or(HostProblem::InfrastructureFailure)?),
            Arc::clone(store),
            CoordinatorLimits::default(),
        );
        let outcome = coordinator.execute_with_control(&mut machine, &invocation, || {
            self.observe_execution_control(&invocation)
        });
        let cursor_result = persist_batch_file_cursors(
            store.as_ref(),
            &cursor_key,
            machine.dataset_cursors(),
            cursor_version,
        );
        // A secondary persistence failure must not erase an in-doubt effect.
        if matches!(&outcome, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
        {
            return Err(HostProblem::UnknownOutcome);
        }
        cursor_result?;
        match outcome {
            ExecutionOutcome::Completed(_) => {
                let mut values = machine
                    .linkage_values()
                    .map_err(|_| HostProblem::ProviderFailure)?;
                values.truncate(call_values.len());
                let result =
                    encode_cobol_call_result(&values).map_err(|_| HostProblem::UnknownOutcome)?;
                writes.extend(lease.completed(store.as_ref(), &machine)?);
                Ok(result)
            }
            ExecutionOutcome::Condition(condition) => Err(HostProblem::Condition {
                name: condition.name,
                response: condition.response,
                response2: condition.response2,
            }),
            ExecutionOutcome::Cancelled => Err(HostProblem::Cancelled),
            ExecutionOutcome::TimedOut => Err(HostProblem::TimedOut),
            ExecutionOutcome::ResourceExhausted(_) => Err(HostProblem::ResourceExhausted),
            ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome() => {
                Err(HostProblem::UnknownOutcome)
            }
            ExecutionOutcome::ProviderFailure(problem) => Err(HostProblem::Condition {
                name: format!("BATCH-PROVIDER:{}", problem.public_message),
                response: -6,
                response2: 0,
            }),
            ExecutionOutcome::InfrastructureFailure(_) => Err(HostProblem::InfrastructureFailure),
            ExecutionOutcome::Abend(_) => Err(HostProblem::Condition {
                name: "INSTALLED-CALL-ABEND".into(),
                response: -1,
                response2: 0,
            }),
            ExecutionOutcome::Rejected(problem) => Err(HostProblem::Condition {
                name: format!("INSTALLED-CALL-REJECTED:{}", problem.public_message),
                response: -2,
                response2: 0,
            }),
            ExecutionOutcome::Suspended(_) => Err(HostProblem::Condition {
                name: "INSTALLED-CALL-SUSPENDED".into(),
                response: -3,
                response2: 0,
            }),
            ExecutionOutcome::Invoke(_) => Err(HostProblem::Condition {
                name: "INSTALLED-CALL-INVOKE".into(),
                response: -4,
                response2: 0,
            }),
            ExecutionOutcome::Transfer(_) => Err(HostProblem::Condition {
                name: "INSTALLED-CALL-TRANSFER".into(),
                response: -5,
                response2: 0,
            }),
        }
    }

    fn execute_runtime_service(
        &self,
        selector: &RuntimeServiceSelector,
        payload: &BoundedPayload,
    ) -> Result<BoundedPayload, HostProblem> {
        if selector.kind != RuntimeServiceKind::LanguageEnvironment || selector.abi_version != 1 {
            return Err(HostProblem::Unsupported);
        }
        match system_service_program(selector.name.as_str()).ok_or(HostProblem::Unsupported)? {
            SystemServiceProgram::Ceedays => execute_ceedays(payload),
            SystemServiceProgram::Mvswait => execute_mvswait(payload),
            SystemServiceProgram::Cobdatft => execute_cobdatft(payload),
            SystemServiceProgram::Cee3abd => execute_cee3abd(payload),
        }
    }

    fn execute_installed_batch(
        &self,
        parent: &Invocation,
        program: &str,
        payload: &BoundedPayload,
        identity: &str,
    ) -> Result<ProgramOutput, HostProblem> {
        let input: ProgramInput =
            serde_json::from_slice(payload.bytes()).map_err(|_| HostProblem::Malformed)?;
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let catalog = store
            .get_provider_state("batch-program", &program.to_ascii_uppercase())
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or(HostProblem::NotFound)?;
        let artifact = ArtifactRef::new(
            String::from_utf8(catalog.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let record = artifacts
            .get_artifact(&artifact)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or(HostProblem::NotFound)?;
        let limits = InvocationLimits::default();
        let sequence = identity;
        let mut bindings = parent.bindings.clone();
        for dd in &input.dds {
            if let Some(dataset) = &dd.dataset {
                bindings.insert(
                    format!("cobol.dd.{}", dd.name.to_ascii_uppercase()),
                    BoundedPayload::new(
                        "mainframe-env.dataset-name@1",
                        dataset.as_bytes().to_vec(),
                        limits,
                    )
                    .map_err(|_| HostProblem::ResourceExhausted)?,
                );
                if let Some(ccsid) = dd.ccsid {
                    bindings.insert(
                        format!("cobol.dd.{}.ccsid", dd.name.to_ascii_uppercase()),
                        BoundedPayload::new(
                            "mainframe-env.ccsid@1",
                            ccsid.to_string().into_bytes(),
                            limits,
                        )
                        .map_err(|_| HostProblem::ResourceExhausted)?,
                    );
                }
            }
            if dd.name.eq_ignore_ascii_case("SYSIN") && !dd.inline_data.is_empty() {
                bindings.insert(
                    "cobol.terminal.input".into(),
                    BoundedPayload::new(
                        "mainframe-env.terminal.input@1",
                        dd.inline_data.clone(),
                        limits,
                    )
                    .map_err(|_| HostProblem::ResourceExhausted)?,
                );
            }
        }
        let invocation = Invocation::new(
            RequestId::new(format!("batch-installed-request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("batch-installed-execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.run_unit_id.clone(),
            Some(parent.execution_id.clone()),
            Selector::new(format!("program:{}", program.to_ascii_uppercase()), limits)
                .map_err(|_| HostProblem::Malformed)?,
            artifact,
            Principal::new(
                parent.principal.id().clone(),
                parent.principal.grants().clone(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.service_class,
            parent.priority,
            parent.deadline_tick,
            TraceId::new(format!("batch-installed-trace-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("batch-installed-effect-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.attempt,
            parent.limits,
            bindings,
            limits,
        )
        .and_then(|invocation| {
            invocation.with_provider_generations(parent.provider_generations.clone(), limits)
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut invocation = with_compatible_runtime_services(invocation)?;
        invocation.cancellation = parent.cancellation.clone();
        let mut machine = ReferenceMachine::from_binary(
            &record.payload,
            invocation.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        install_batch_environment(&mut machine, &input)?;
        let coordinator = ExecutionCoordinator::durable(
            Arc::clone(self.host.get().ok_or(HostProblem::InfrastructureFailure)?),
            Arc::clone(self.store.get().ok_or(HostProblem::InfrastructureFailure)?),
            CoordinatorLimits::default(),
        );
        match coordinator.execute_with_control(&mut machine, &invocation, || {
            self.observe_execution_control(&invocation)
        }) {
            ExecutionOutcome::Completed(completion) => Ok(ProgramOutput {
                return_code: completion.return_code,
                records: completion
                    .output
                    .bytes()
                    .split(|byte| *byte == b'\n')
                    .filter(|record| !record.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect(),
                dd_outputs: BTreeMap::new(),
            }),
            ExecutionOutcome::Condition(condition) => Ok(ProgramOutput {
                return_code: condition.response,
                records: vec![condition.name.into_bytes()],
                dd_outputs: BTreeMap::new(),
            }),
            ExecutionOutcome::Abend(abend) => Err(HostProblem::Condition {
                name: format!("ABEND:{}", abend.code),
                response: -1,
                response2: 0,
            }),
            ExecutionOutcome::Cancelled => Err(HostProblem::Cancelled),
            ExecutionOutcome::TimedOut => Err(HostProblem::TimedOut),
            ExecutionOutcome::ResourceExhausted(_) => Err(HostProblem::ResourceExhausted),
            ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome() => {
                Err(HostProblem::UnknownOutcome)
            }
            ExecutionOutcome::ProviderFailure(problem) => Err(HostProblem::Condition {
                name: format!("BATCH-PROVIDER:{}", problem.public_message),
                response: -6,
                response2: 0,
            }),
            ExecutionOutcome::InfrastructureFailure(_) => Err(HostProblem::InfrastructureFailure),
            ExecutionOutcome::Rejected(problem) => Err(HostProblem::Condition {
                name: format!(
                    "BATCH-INSTALLED-REJECTED:{} at {}",
                    problem.public_message,
                    machine.position_summary()
                ),
                response: -2,
                response2: 0,
            }),
            ExecutionOutcome::Suspended(_) => Err(HostProblem::Condition {
                name: "BATCH-INSTALLED-SUSPENDED".into(),
                response: -3,
                response2: 0,
            }),
            ExecutionOutcome::Invoke(_) => Err(HostProblem::Condition {
                name: "BATCH-INSTALLED-INVOKE".into(),
                response: -4,
                response2: 0,
            }),
            ExecutionOutcome::Transfer(_) => Err(HostProblem::Condition {
                name: "BATCH-INSTALLED-TRANSFER".into(),
                response: -5,
                response2: 0,
            }),
        }
    }
}

impl Program for CobolProgram {
    fn execute(
        &self,
        parent: &Invocation,
        input: &ProgramInput,
    ) -> Result<ProgramOutput, HostProblem> {
        let source = source_bundle(input)?;
        let compiled = CobolCompiler::default()
            .compile(CompilerRequest {
                source,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").map_err(|_| HostProblem::Malformed)?,
                options: CompileOptions::new(BTreeMap::new())
                    .map_err(|_| HostProblem::Malformed)?,
            })
            .map_err(|_| HostProblem::ProviderFailure)?;
        let CompilerResult::Published { artifact, .. } = compiled else {
            return Err(HostProblem::Malformed);
        };
        let limits = InvocationLimits::default();
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let invocation = Invocation::new(
            RequestId::new(
                format!(
                    "batch-cobol-request-{}-{sequence}",
                    parent.execution_id.as_str()
                ),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(
                format!(
                    "batch-cobol-execution-{}-{sequence}",
                    parent.execution_id.as_str()
                ),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new(
                format!(
                    "batch-cobol-run-{}-{sequence}",
                    parent.execution_id.as_str()
                ),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            Some(parent.execution_id.clone()),
            Selector::new("program:COBOL", limits).map_err(|_| HostProblem::Malformed)?,
            ArtifactRef::new(format!("sha256:{}", artifact.id().to_hex()), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            Principal::new(
                parent.principal.id().clone(),
                parent.principal.grants().clone(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.service_class,
            parent.priority,
            parent.deadline_tick,
            TraceId::new(
                format!(
                    "batch-cobol-trace-{}-{sequence}",
                    parent.execution_id.as_str()
                ),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(
                format!(
                    "batch-cobol-effect-{}-{sequence}",
                    parent.execution_id.as_str()
                ),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.attempt,
            parent.limits,
            parent.bindings.clone(),
            limits,
        )
        .and_then(|invocation| {
            invocation.with_provider_generations(parent.provider_generations.clone(), limits)
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut invocation = with_compatible_runtime_services(invocation)?;
        invocation.cancellation = parent.cancellation.clone();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        install_batch_environment(&mut machine, input)?;
        let coordinator = match (self.host.get(), self.store.get()) {
            (Some(host), Some(store)) => ExecutionCoordinator::durable(
                Arc::clone(host),
                Arc::clone(store),
                CoordinatorLimits::default(),
            ),
            (Some(host), None) => {
                ExecutionCoordinator::with_host(Arc::clone(host), CoordinatorLimits::default())
            }
            (None, _) => ExecutionCoordinator::local(CoordinatorLimits::default()),
        };
        match coordinator.execute_with_control(&mut machine, &invocation, || {
            self.observe_execution_control(&invocation)
        }) {
            ExecutionOutcome::Completed(completion) => {
                self.finish_run_unit(&invocation)?;
                Ok(ProgramOutput {
                    return_code: completion.return_code,
                    records: completion
                        .output
                        .bytes()
                        .split(|byte| *byte == b'\n')
                        .filter(|record| !record.is_empty())
                        .map(<[u8]>::to_vec)
                        .collect(),
                    dd_outputs: BTreeMap::new(),
                })
            }
            ExecutionOutcome::Condition(condition) => Ok(ProgramOutput {
                return_code: condition.response,
                records: vec![condition.name.into_bytes()],
                dd_outputs: BTreeMap::new(),
            }),
            ExecutionOutcome::Abend(_) => Err(HostProblem::Condition {
                name: "ABEND".into(),
                response: -1,
                response2: 0,
            }),
            ExecutionOutcome::Cancelled => Err(HostProblem::Cancelled),
            ExecutionOutcome::TimedOut => Err(HostProblem::TimedOut),
            ExecutionOutcome::ResourceExhausted(_) => Err(HostProblem::ResourceExhausted),
            ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome() => {
                Err(HostProblem::UnknownOutcome)
            }
            ExecutionOutcome::ProviderFailure(_) => Err(HostProblem::ProviderFailure),
            ExecutionOutcome::InfrastructureFailure(_) => Err(HostProblem::InfrastructureFailure),
            ExecutionOutcome::Rejected(_)
            | ExecutionOutcome::Suspended(_)
            | ExecutionOutcome::Invoke(_)
            | ExecutionOutcome::Transfer(_) => Err(HostProblem::Unsupported),
        }
    }
}

fn install_batch_environment(
    machine: &mut ReferenceMachine,
    input: &ProgramInput,
) -> Result<(), HostProblem> {
    let Some(execution) = &input.execution else {
        return Ok(());
    };
    machine
        .install_mvs_tiot(
            &execution.job_name,
            &execution.step_name,
            input.dds.iter().map(|dd| dd.name.as_str()),
        )
        .map(|_| ())
        .map_err(|problem| match problem {
            mainframe_env_interpreter::MachineProblem::ResourceExhausted => {
                HostProblem::ResourceExhausted
            }
            _ => HostProblem::Malformed,
        })
}

fn decode_cobol_call_values(payload: &BoundedPayload) -> Result<Vec<Vec<u8>>, HostProblem> {
    if payload.schema() != "mainframe-env.cobol.call@1" {
        return Err(HostProblem::Malformed);
    }
    let bytes = payload.bytes();
    let mut at = 0usize;
    let take = |at: &mut usize, amount: usize| -> Result<&[u8], HostProblem> {
        let end = at
            .checked_add(amount)
            .ok_or(HostProblem::ResourceExhausted)?;
        let value = bytes.get(*at..end).ok_or(HostProblem::Malformed)?;
        *at = end;
        Ok(value)
    };
    let count = usize::try_from(u32::from_be_bytes(
        take(&mut at, 4)?
            .try_into()
            .map_err(|_| HostProblem::Malformed)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if count > InvocationLimits::default().max_bindings {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let name_length = usize::try_from(u64::from_be_bytes(
            take(&mut at, 8)?
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let _name = take(&mut at, name_length)?;
        if take(&mut at, 1)? != [1] {
            return Err(HostProblem::Malformed);
        }
        let value_length = usize::try_from(u64::from_be_bytes(
            take(&mut at, 8)?
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        values.push(take(&mut at, value_length)?.to_vec());
    }
    if at != bytes.len() {
        return Err(HostProblem::Malformed);
    }
    Ok(values)
}

fn execute_ceedays(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let mut values = decode_cobol_call_values(payload)?;
    if values.len() != 4 {
        return Err(HostProblem::Malformed);
    }
    let date = cee_vstring(&values[0]).ok_or(HostProblem::Malformed)?;
    let picture = cee_vstring(&values[1]).ok_or(HostProblem::Malformed)?;
    let parsed = parse_ceedays_date(date, picture);
    values[2].fill(0);
    values[3].fill(0);
    if let Some((year, month, day)) = parsed {
        let lilian = civil_day(year, month, day) - civil_day(1582, 10, 14);
        let lilian = i32::try_from(lilian).map_err(|_| HostProblem::ResourceExhausted)?;
        if values[2].len() != 4 {
            return Err(HostProblem::Malformed);
        }
        values[2].copy_from_slice(&lilian.to_be_bytes());
    } else {
        if values[3].len() < 8 {
            return Err(HostProblem::Malformed);
        }
        values[3][1] = 3;
        values[3][3] = 1;
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

fn execute_mvswait(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let values = decode_cobol_call_values(payload)?;
    if values.len() != 1 || values[0].len() != 4 {
        return Err(HostProblem::Malformed);
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

fn execute_cobdatft(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let mut values = decode_cobol_call_values(payload)?;
    if values.len() != 1 || values[0].len() < 80 {
        return Err(HostProblem::Malformed);
    }
    let record = &mut values[0];
    let input = record[1..21].to_vec();
    let valid = match (record[0], record[21]) {
        (b'1', b'1') if input.get(4) != Some(&b'-') => {
            record[22..26].copy_from_slice(&input[..4]);
            record[26] = b'-';
            record[27..29].copy_from_slice(&input[4..6]);
            record[29] = b'-';
            record[30..32].copy_from_slice(&input[6..8]);
            true
        }
        (b'2', b'2') => {
            record[22..26].copy_from_slice(&input[..4]);
            record[26..28].copy_from_slice(&input[5..7]);
            record[28..30].copy_from_slice(&input[8..10]);
            true
        }
        _ => false,
    };
    if !valid {
        record[42..55].copy_from_slice(b"INVALID INPUT");
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

fn execute_cee3abd(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let values = decode_cobol_call_values(payload)?;
    if values.len() > 2 || values.iter().any(|value| value.len() != 4) {
        return Err(HostProblem::Malformed);
    }
    let code = values.first().map_or(999, |value| {
        i32::from_be_bytes(value.as_slice().try_into().unwrap_or(999i32.to_be_bytes()))
    });
    BoundedPayload::new(
        "mainframe-env.program.abend@1",
        format!("U{:04}", code.unsigned_abs().min(9999)).into_bytes(),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

fn cee_vstring(value: &[u8]) -> Option<&[u8]> {
    let length = usize::try_from(i16::from_be_bytes(value.get(..2)?.try_into().ok()?)).ok()?;
    value.get(2..2usize.checked_add(length)?)
}

#[cfg(test)]
fn valid_ceedays_date(value: &[u8], picture: &[u8]) -> bool {
    parse_ceedays_date(value, picture).is_some()
}

fn parse_ceedays_date(value: &[u8], picture: &[u8]) -> Option<(u32, u32, u32)> {
    let value = trim_ascii(value);
    let picture = trim_ascii(picture);
    let (year, month, day) = match picture {
        b"YYYY-MM-DD"
            if value.len() == 10 && value.get(4) == Some(&b'-') && value.get(7) == Some(&b'-') =>
        {
            (&value[..4], &value[5..7], &value[8..])
        }
        b"YYYYMMDD" if value.len() == 8 => (&value[..4], &value[4..6], &value[6..]),
        _ => return None,
    };
    let number = |bytes: &[u8]| -> Option<u32> {
        bytes.iter().try_fold(0u32, |value, byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(*byte - b'0'))
        })
    };
    let year = number(year)?;
    let month = number(month)?;
    let day = number(day)?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (year <= 9999 && (year, month, day) >= (1582, 10, 15) && day >= 1 && day <= days)
        .then_some((year, month, day))
}

fn civil_day(year: u32, month: u32, day: u32) -> i64 {
    let mut year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|byte| matches!(*byte, 0 | b' ')) {
        value = &value[1..];
    }
    while value.last().is_some_and(|byte| matches!(*byte, 0 | b' ')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn source_bundle(input: &ProgramInput) -> Result<SourceBundle, HostProblem> {
    let source = input
        .dds
        .iter()
        .find(|dd| dd.name == "SYSIN")
        .map(|dd| dd.inline_data.clone())
        .ok_or(HostProblem::NotFound)?;
    let source_limits = SourceLimits::default();
    let path = LogicalPath::new("SYSIN.cbl", source_limits.max_path_bytes)
        .map_err(|_| HostProblem::Malformed)?;
    let format = if input.parameter.as_deref().is_some_and(|parameters| {
        parameters
            .split(',')
            .any(|item| item.trim().eq_ignore_ascii_case("FORMAT=FIXED"))
    }) {
        SourceFormat::Fixed
    } else {
        SourceFormat::Free
    };
    let primary = SourceFile::input(
        "SYSIN.cbl",
        source,
        format,
        SourceEncoding::Utf8,
        source_limits,
    )
    .map_err(|_| HostProblem::Malformed)?;
    let library_dds = input
        .dds
        .iter()
        .filter(|dd| dd.name.starts_with("SYSLIB"))
        .collect::<Vec<_>>();
    if library_dds.is_empty() {
        return SourceBundle::new(
            &path,
            vec![primary],
            BTreeMap::new(),
            Vec::new(),
            source_limits,
        )
        .map_err(|_| HostProblem::Malformed);
    }
    let mut files = vec![primary];
    let mut libraries = Vec::with_capacity(library_dds.len() + 1);
    for (index, dd) in library_dds.into_iter().enumerate() {
        let member = dd.dataset.as_deref().ok_or(HostProblem::Malformed)?;
        let leaf = member
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .ok_or(HostProblem::Malformed)?;
        let logical = format!("jes/{index:03}/{leaf}");
        let member_path = LogicalPath::new(&logical, source_limits.max_path_bytes)
            .map_err(|_| HostProblem::Malformed)?;
        files.push(
            SourceFile::input(
                logical,
                dd.inline_data.clone(),
                format,
                SourceEncoding::Utf8,
                source_limits,
            )
            .map_err(|_| HostProblem::Malformed)?,
        );
        libraries.push(
            SourceLibrary::new(format!("jes-{index:03}"), vec![member_path], source_limits)
                .map_err(|_| HostProblem::Malformed)?,
        );
    }
    let abi = materialize_host_abi_libraries(
        &[cics_abi_library(), db2_abi_library(), mq_abi_library()],
        source_limits,
    )
    .map_err(|_| HostProblem::Malformed)?;
    files.extend(abi.files);
    libraries.extend(abi.libraries);
    SourceBundle::with_libraries(
        &path,
        files,
        libraries,
        BTreeMap::new(),
        Vec::new(),
        source_limits,
    )
    .map_err(|_| HostProblem::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_batch::DdPlan;
    use mainframe_env_execution_api::{PrincipalId, ResourceLimits, ServiceClass};
    use mainframe_env_host_api::{HostLimits, RegistrySnapshot};
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::{ExecutionState, PlatformStore};
    use std::collections::BTreeSet;

    fn parent() -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
            RequestId::new("parent-request", limits).unwrap(),
            ExecutionId::new("parent-execution", limits).unwrap(),
            RunUnitId::new("parent-run", limits).unwrap(),
            None,
            Selector::new("program:test", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("BATCH", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            u64::MAX,
            TraceId::new("parent-trace", limits).unwrap(),
            IdempotencyKey::new("parent-key", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    fn call_payload(values: &[Vec<u8>]) -> BoundedPayload {
        let mut bytes = u32::try_from(values.len()).unwrap().to_be_bytes().to_vec();
        for (index, value) in values.iter().enumerate() {
            let name = format!("ARG{}", index + 1);
            bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(1);
            bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
            bytes.extend_from_slice(value);
        }
        BoundedPayload::new(
            "mainframe-env.cobol.call@1",
            bytes,
            InvocationLimits::default(),
        )
        .unwrap()
    }

    fn vstring(value: &[u8]) -> Vec<u8> {
        let mut output = i16::try_from(value.len()).unwrap().to_be_bytes().to_vec();
        output.extend_from_slice(value);
        output
    }

    fn call_result(payload: &BoundedPayload) -> Vec<Vec<u8>> {
        assert_eq!(payload.schema(), "mainframe-env.cobol.call-result@1");
        let mut at = 0usize;
        let count = u32::from_be_bytes(payload.bytes()[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let length =
                u64::from_be_bytes(payload.bytes()[at..at + 8].try_into().unwrap()) as usize;
            at += 8;
            values.push(payload.bytes()[at..at + length].to_vec());
            at += length;
        }
        assert_eq!(at, payload.bytes().len());
        values
    }

    #[test]
    fn runtime_services_route_by_exact_typed_selector_not_program_name() {
        let router = default_program_router();
        let invocation = parent();
        let request = |selector: RuntimeServiceSelector| EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: invocation.deadline_tick,
            idempotency_key: None,
            request: HostRequest::Program(ProgramRequest::Call {
                program: mainframe_env_host_api::ProgramName::new("APPLICATION", 128).unwrap(),
                payload: call_payload(&[0i32.to_be_bytes().to_vec()]),
                service: Some(selector),
            }),
        };
        let selected = router.invoke(
            &invocation,
            request(RuntimeServiceSelector {
                kind: RuntimeServiceKind::LanguageEnvironment,
                name: mainframe_env_host_api::RuntimeServiceName::new("MVSWAIT", 128).unwrap(),
                abi_version: 1,
            }),
        );
        let HostResult::Program(payload) = selected.outcome.unwrap() else {
            panic!("typed runtime service did not return a program result")
        };
        assert_eq!(call_result(&payload), [0i32.to_be_bytes().to_vec()]);

        let rejected = router.invoke(
            &invocation,
            request(RuntimeServiceSelector {
                kind: RuntimeServiceKind::HostExtension,
                name: mainframe_env_host_api::RuntimeServiceName::new("MVSWAIT", 128).unwrap(),
                abi_version: 1,
            }),
        );
        assert_eq!(rejected.outcome, Err(HostProblem::Unsupported));
    }

    #[test]
    fn compatible_le_bindings_are_explicit_and_conflicts_fail_closed() {
        let mut invocation = parent();
        bind_compatible_runtime_services(&mut invocation).unwrap();
        for name in compatible_system_services() {
            let binding = &invocation.bindings[&format!("cobol.runtime-service.{name}")];
            assert_eq!(binding.schema(), "mainframe-env.runtime-service-selector@1");
            assert_eq!(binding.bytes(), format!("le:{name}:1").as_bytes());
        }
        invocation.bindings.insert(
            "cobol.runtime-service.MVSWAIT".into(),
            BoundedPayload::new(
                "mainframe-env.runtime-service-selector@1",
                b"extension:MVSWAIT:1".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            bind_compatible_runtime_services(&mut invocation),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn default_cobol_program_compiles_and_runs_reference_machine() {
        let program = CobolProgram::new();
        let output = program
            .execute(&parent(), &ProgramInput {
                parameter: None,
                dds: vec![DdPlan {
                    name: "SYSIN".into(),
                    dataset: None,
                    member: None,
                    generation: None,
                    organization: None,
                    record_format: None,
                    logical_record_length: None,
                    ccsid: None,
                    temporary: false,
                    sysout: None,
                    disposition: Vec::new(),
                    inline_data: b"IDENTIFICATION DIVISION.\nPROGRAM-ID. BATCH.\nPROCEDURE DIVISION.\nDISPLAY 'BATCH COBOL'.\nSTOP RUN.\n".to_vec(),
                    concatenation: false,
                    source_line: 1,
                    source_end_line: 1,
                    parameters: Vec::new(),
                }],
                dd_records: BTreeMap::new(),
                execution: None,
            })
            .unwrap();
        assert_eq!(output.return_code, 0);
        assert_eq!(output.records, vec![b"BATCH COBOL".to_vec()]);
    }

    #[test]
    fn batch_cobol_installs_a_bounded_mvs_tiot_from_execution_context() {
        let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. TIOTTEST.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 MARKER PIC X(4) VALUE 'KEEP'.\n01 PSAPTR POINTER.\n01 BUMP-TIOT PIC S9(08) BINARY VALUE ZERO.\n01 TIOT-INDEX REDEFINES BUMP-TIOT POINTER.\nLINKAGE SECTION.\n01 PSA-BLOCK.\n  05 FILLER PIC X(16).\n  05 TCB-POINT POINTER.\n01 TCB-BLOCK.\n  05 FILLER PIC X(12).\n  05 TIOT-POINT POINTER.\n01 TIOT-BLOCK.\n  05 TIOTNJOB PIC X(08).\n  05 TIOTJSTP PIC X(08).\n  05 TIOTPSTP PIC X(08).\n01 TIOT-ENTRY.\n  05 TIOT-SEG.\n    10 TIO-LEN PIC X(01).\n    10 FILLER PIC X(03).\n    10 TIOCDDNM PIC X(08).\n    10 FILLER PIC X(05).\n    10 UCB-ADDR PIC X(03).\n      88 NULL-UCB VALUE LOW-VALUES.\n  05 FILLER PIC X(04).\n    88 END-OF-TIOT VALUE LOW-VALUES.\nPROCEDURE DIVISION.\nSET ADDRESS OF PSA-BLOCK TO PSAPTR.\nSET ADDRESS OF TCB-BLOCK TO TCB-POINT.\nSET ADDRESS OF TIOT-BLOCK TO TIOT-POINT.\nSET TIOT-INDEX TO TIOT-POINT.\nDISPLAY TIOTNJOB ':' TIOTJSTP.\nCOMPUTE BUMP-TIOT = BUMP-TIOT + LENGTH OF TIOT-BLOCK.\nSET ADDRESS OF TIOT-ENTRY TO TIOT-INDEX.\nDISPLAY TIOCDDNM.\nDISPLAY MARKER.\nSTOP RUN.\n";
        let program = CobolProgram::new();
        let output = program
            .execute(
                &parent(),
                &ProgramInput {
                    parameter: None,
                    dds: vec![DdPlan {
                        name: "SYSIN".into(),
                        dataset: None,
                        member: None,
                        generation: None,
                        organization: None,
                        record_format: None,
                        logical_record_length: None,
                        ccsid: None,
                        temporary: false,
                        sysout: None,
                        disposition: Vec::new(),
                        inline_data: source.to_vec(),
                        concatenation: false,
                        source_line: 1,
                        source_end_line: 1,
                        parameters: Vec::new(),
                    }],
                    dd_records: BTreeMap::new(),
                    execution: Some(mainframe_env_batch::ProgramExecutionContext {
                        job_name: "TIOTJOB".into(),
                        step_name: "STEP1".into(),
                    }),
                },
            )
            .unwrap();
        assert_eq!(output.return_code, 0);
        assert_eq!(
            output.records,
            vec![
                b"TIOTJOB :STEP1   ".to_vec(),
                b"SYSIN   ".to_vec(),
                b"KEEP".to_vec()
            ]
        );
    }

    #[test]
    fn reached_ceedays_pictures_validate_calendar_dates() {
        assert!(valid_ceedays_date(b"2026-08-30", b"YYYY-MM-DD"));
        assert!(valid_ceedays_date(b"20110422", b"YYYYMMDD"));
        assert!(valid_ceedays_date(b"20240229", b"YYYYMMDD"));
        assert!(!valid_ceedays_date(b"20230229", b"YYYYMMDD"));
        assert!(!valid_ceedays_date(b"20111301", b"YYYYMMDD"));
    }

    #[test]
    fn compatible_date_wait_and_abend_services_match_reached_abis() {
        let mut date_record = vec![b' '; 80];
        date_record[0] = b'1';
        date_record[1..9].copy_from_slice(b"20260830");
        date_record[21] = b'1';
        let converted = execute_cobdatft(&call_payload(&[date_record])).unwrap();
        let converted = call_result(&converted);
        assert_eq!(&converted[0][22..32], b"2026-08-30");
        assert_eq!(&converted[0][42..80], vec![b' '; 38]);

        let days = execute_ceedays(&call_payload(&[
            vstring(b"1988-05-16"),
            vstring(b"YYYY-MM-DD"),
            vec![0; 4],
            vec![0; 12],
        ]))
        .unwrap();
        let days = call_result(&days);
        assert_eq!(
            i32::from_be_bytes(days[2].as_slice().try_into().unwrap()),
            148_138
        );
        assert_eq!(days[3], vec![0; 12]);

        let waited = execute_mvswait(&call_payload(&[36i32.to_be_bytes().to_vec()])).unwrap();
        assert_eq!(call_result(&waited)[0], 36i32.to_be_bytes());

        let abend = execute_cee3abd(&call_payload(&[
            999i32.to_be_bytes().to_vec(),
            0i32.to_be_bytes().to_vec(),
        ]))
        .unwrap();
        assert_eq!(abend.schema(), "mainframe-env.program.abend@1");
        assert_eq!(abend.bytes(), b"U0999");
    }

    #[test]
    fn jes_cobol_route_accepts_fixed_primary_and_ordered_copy_library() {
        let program = CobolProgram::new();
        let output = program
            .execute(
                &parent(),
                &ProgramInput {
                    parameter: Some("FORMAT=FIXED".into()),
                    dds: vec![
                        DdPlan {
                            name: "SYSIN".into(),
                            dataset: None,
                            member: None,
                            generation: None,
                            organization: None,
                            record_format: None,
                            logical_record_length: None,
                            ccsid: None,
                            temporary: false,
                            sysout: None,
                            disposition: Vec::new(),
                            inline_data: b"       IDENTIFICATION DIVISION.\n       PROGRAM-ID. BATCHLIB.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n       COPY MESSAGE.\n       PROCEDURE DIVISION.\n       DISPLAY MESSAGE-TEXT.\n       STOP RUN.\n".to_vec(),
                            concatenation: false,
                            source_line: 1,
                            source_end_line: 1,
                            parameters: Vec::new(),
                        },
                        DdPlan {
                            name: "SYSLIB".into(),
                            dataset: Some("MESSAGE.cpy".into()),
                            member: None,
                            generation: None,
                            organization: None,
                            record_format: None,
                            logical_record_length: None,
                            ccsid: None,
                            temporary: false,
                            sysout: None,
                            disposition: Vec::new(),
                            inline_data: b"       01 MESSAGE-TEXT PIC X(5) VALUE 'HELLO'.\n".to_vec(),
                            concatenation: false,
                            source_line: 9,
                            source_end_line: 9,
                            parameters: Vec::new(),
                        },
                    ],
                    dd_records: BTreeMap::new(),
                    execution: None,
                },
            )
            .unwrap();
        assert_eq!(output.return_code, 0);
        assert_eq!(output.records, vec![b"HELLO".to_vec()]);
    }

    #[test]
    fn composed_cobol_execution_commits_projection_events_and_outbox() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        let host = Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, Vec::new(), InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        ));
        let program = CobolProgram::new();
        assert!(program.host.set(host).is_ok());
        assert!(program.store.set(store.clone()).is_ok());
        let output = program
            .execute(&parent(), &ProgramInput {
                parameter: None,
                dds: vec![DdPlan {
                    name: "SYSIN".into(),
                    dataset: None,
                    member: None,
                    generation: None,
                    organization: None,
                    record_format: None,
                    logical_record_length: None,
                    ccsid: None,
                    temporary: false,
                    sysout: None,
                    disposition: Vec::new(),
                    inline_data: b"IDENTIFICATION DIVISION.\nPROGRAM-ID. DURABLE.\nPROCEDURE DIVISION.\nDISPLAY 'DURABLE'.\nSTOP RUN.\n".to_vec(),
                    concatenation: false,
                    source_line: 1,
                    source_end_line: 1,
                    parameters: Vec::new(),
                }],
                dd_records: BTreeMap::new(),
                execution: None,
            })
            .unwrap();
        assert_eq!(output.records, vec![b"DURABLE".to_vec()]);
        let execution = ExecutionId::new(
            "batch-cobol-execution-parent-execution-1",
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            store.get_execution(&execution).unwrap().unwrap().state,
            ExecutionState::Completed
        );
        assert_eq!(store.events(&execution, 1, 16).unwrap().len(), 5);
        assert_eq!(store.pending_notifications(16).unwrap().len(), 5);
    }
}

#[cfg(test)]
mod hardening;

mod replay;

mod instance;
