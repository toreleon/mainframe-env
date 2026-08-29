use mainframe_env_batch::{Program, ProgramInput, ProgramOutput, ProgramRouter};
use mainframe_env_compiler::{CobolCompiler, owned_compatibility_library};
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, ExecutionOutcome, IdempotencyKey, Invocation, InvocationLimits,
    Principal, RequestId, RunUnitId, Selector, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, ScopedHostService,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use mainframe_env_store_api::PlatformStore;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub struct DefaultProgramRouter {
    router: Arc<ProgramRouter>,
    cobol: Arc<CobolProgram>,
}

impl DefaultProgramRouter {
    pub(crate) fn bind_runtime(
        &self,
        host: Arc<ScopedHostService>,
        store: Arc<dyn PlatformStore>,
    ) -> Result<(), HostProblem> {
        self.cobol
            .host
            .set(host)
            .map_err(|_| HostProblem::IdempotencyConflict)?;
        self.cobol
            .store
            .set(store)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }
}

impl HostProvider for DefaultProgramRouter {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.router.descriptor()
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
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

struct CobolProgram {
    host: OnceLock<Arc<ScopedHostService>>,
    store: OnceLock<Arc<dyn PlatformStore>>,
    sequence: AtomicU64,
}

impl CobolProgram {
    const fn new() -> Self {
        Self {
            host: OnceLock::new(),
            store: OnceLock::new(),
            sequence: AtomicU64::new(1),
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
            RequestId::new(format!("batch-cobol-request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("batch-cobol-execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new(format!("batch-cobol-run-{sequence}"), limits)
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
            TraceId::new(format!("batch-cobol-trace-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("batch-cobol-effect-{sequence}"), limits)
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
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
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
        match coordinator.execute(&mut machine, &invocation, ExecutionControl::default()) {
            ExecutionOutcome::Completed(completion) => Ok(ProgramOutput {
                return_code: completion.return_code,
                records: completion
                    .output
                    .bytes()
                    .split(|byte| *byte == b'\n')
                    .filter(|record| !record.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect(),
            }),
            ExecutionOutcome::Condition(condition) => Ok(ProgramOutput {
                return_code: condition.response,
                records: vec![condition.name.into_bytes()],
            }),
            ExecutionOutcome::Abend(_) => Err(HostProblem::Condition {
                name: "ABEND".into(),
                response: -1,
                response2: 0,
            }),
            ExecutionOutcome::Cancelled => Err(HostProblem::Cancelled),
            ExecutionOutcome::TimedOut => Err(HostProblem::TimedOut),
            ExecutionOutcome::ResourceExhausted(_) => Err(HostProblem::ResourceExhausted),
            ExecutionOutcome::ProviderFailure(_) => Err(HostProblem::ProviderFailure),
            ExecutionOutcome::InfrastructureFailure(_) => Err(HostProblem::InfrastructureFailure),
            ExecutionOutcome::Rejected(_)
            | ExecutionOutcome::Suspended(_)
            | ExecutionOutcome::Invoke(_)
            | ExecutionOutcome::Transfer(_) => Err(HostProblem::Unsupported),
        }
    }
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
    let (compatibility, compatibility_library) =
        owned_compatibility_library(source_limits).map_err(|_| HostProblem::Malformed)?;
    files.extend(compatibility);
    libraries.push(compatibility_library);
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
            1000,
            TraceId::new("parent-trace", limits).unwrap(),
            IdempotencyKey::new("parent-key", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
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
                    temporary: false,
                    sysout: None,
                    disposition: Vec::new(),
                    inline_data: b"IDENTIFICATION DIVISION.\nPROGRAM-ID. BATCH.\nPROCEDURE DIVISION.\nDISPLAY 'BATCH COBOL'.\nSTOP RUN.\n".to_vec(),
                    concatenation: false,
                    source_line: 1,
                }],
            })
            .unwrap();
        assert_eq!(output.return_code, 0);
        assert_eq!(output.records, vec![b"BATCH COBOL".to_vec()]);
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
                            temporary: false,
                            sysout: None,
                            disposition: Vec::new(),
                            inline_data: b"       IDENTIFICATION DIVISION.\n       PROGRAM-ID. BATCHLIB.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n       COPY MESSAGE.\n       PROCEDURE DIVISION.\n       DISPLAY MESSAGE-TEXT.\n       STOP RUN.\n".to_vec(),
                            concatenation: false,
                            source_line: 1,
                        },
                        DdPlan {
                            name: "SYSLIB".into(),
                            dataset: Some("MESSAGE.cpy".into()),
                            temporary: false,
                            sysout: None,
                            disposition: Vec::new(),
                            inline_data: b"       01 MESSAGE-TEXT PIC X(5) VALUE 'HELLO'.\n".to_vec(),
                            concatenation: false,
                            source_line: 9,
                        },
                    ],
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
                    temporary: false,
                    sysout: None,
                    disposition: Vec::new(),
                    inline_data: b"IDENTIFICATION DIVISION.\nPROGRAM-ID. DURABLE.\nPROCEDURE DIVISION.\nDISPLAY 'DURABLE'.\nSTOP RUN.\n".to_vec(),
                    concatenation: false,
                    source_line: 1,
                }],
            })
            .unwrap();
        assert_eq!(output.records, vec![b"DURABLE".to_vec()]);
        let execution =
            ExecutionId::new("batch-cobol-execution-1", InvocationLimits::default()).unwrap();
        assert_eq!(
            store.get_execution(&execution).unwrap().unwrap().state,
            ExecutionState::Completed
        );
        assert_eq!(store.events(&execution, 1, 16).unwrap().len(), 5);
        assert_eq!(store.pending_notifications(16).unwrap().len(), 5);
    }
}
