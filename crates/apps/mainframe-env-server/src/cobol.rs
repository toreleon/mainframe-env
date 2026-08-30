use mainframe_env_batch::{Program, ProgramInput, ProgramOutput, ProgramRouter};
use mainframe_env_compiler::{CobolCompiler, owned_compatibility_library};
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, ExecutionOutcome, IdempotencyKey, Invocation,
    InvocationLimits, Principal, RequestId, RunUnitId, Selector, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, ProgramRequest, ScopedHostService,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
    encode_cobol_call_result,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{ArtifactStore, PlatformStore};
use std::collections::BTreeMap;
use std::path::Path;
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
        if let HostRequest::Program(ProgramRequest::Call { program, payload }) = &effect.request
            && payload.schema() == "mainframe-env.cobol.call@1"
        {
            return EffectResult {
                sequence: effect.sequence,
                outcome: self
                    .cobol
                    .execute_installed(invocation, program.as_str(), payload)
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

struct CobolProgram {
    host: OnceLock<Arc<ScopedHostService>>,
    store: OnceLock<Arc<dyn PlatformStore>>,
    artifacts: OnceLock<LocalArtifactStore>,
    sequence: AtomicU64,
}

impl CobolProgram {
    const fn new() -> Self {
        Self {
            host: OnceLock::new(),
            store: OnceLock::new(),
            artifacts: OnceLock::new(),
            sequence: AtomicU64::new(1),
        }
    }

    fn execute_installed(
        &self,
        parent: &Invocation,
        program: &str,
        payload: &BoundedPayload,
    ) -> Result<BoundedPayload, HostProblem> {
        if program.eq_ignore_ascii_case("CEEDAYS") {
            return execute_ceedays(payload);
        }
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let artifacts = self
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let catalog = store
            .get_provider_state("online-program", &program.to_ascii_uppercase())
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
        let call_values = decode_cobol_call_values(payload)?;
        let limits = InvocationLimits::default();
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let mut bindings = parent.bindings.clone();
        bindings.insert("cobol.call.arguments".into(), payload.clone());
        let invocation = Invocation::new(
            RequestId::new(format!("online-call-request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("online-call-execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new(format!("online-call-run-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
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
        let mut machine = ReferenceMachine::from_binary(
            &record.payload,
            invocation.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::ProviderFailure)?;
        let coordinator = ExecutionCoordinator::with_host(
            Arc::clone(self.host.get().ok_or(HostProblem::InfrastructureFailure)?),
            CoordinatorLimits::default(),
        );
        match coordinator.execute(&mut machine, &invocation, ExecutionControl::default()) {
            ExecutionOutcome::Completed(_) => {
                let mut values = machine
                    .linkage_values()
                    .map_err(|_| HostProblem::ProviderFailure)?;
                values.truncate(call_values.len());
                encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
            }
            ExecutionOutcome::Condition(condition) => Err(HostProblem::Condition {
                name: condition.name,
                response: condition.response,
                response2: condition.response2,
            }),
            ExecutionOutcome::Cancelled => Err(HostProblem::Cancelled),
            ExecutionOutcome::TimedOut => Err(HostProblem::TimedOut),
            ExecutionOutcome::ResourceExhausted(_) => Err(HostProblem::ResourceExhausted),
            ExecutionOutcome::ProviderFailure(_) => Err(HostProblem::ProviderFailure),
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
    let valid = valid_ceedays_date(date, picture);
    values[2].fill(0);
    values[3].fill(0);
    if !valid {
        if values[3].len() < 8 {
            return Err(HostProblem::Malformed);
        }
        values[3][1] = 3;
        values[3][3] = 1;
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

fn cee_vstring(value: &[u8]) -> Option<&[u8]> {
    let length = usize::try_from(i16::from_be_bytes(value.get(..2)?.try_into().ok()?)).ok()?;
    value.get(2..2usize.checked_add(length)?)
}

fn valid_ceedays_date(value: &[u8], picture: &[u8]) -> bool {
    let value = trim_ascii(value);
    let picture = trim_ascii(picture);
    let (year, month, day) = match picture {
        b"YYYY-MM-DD"
            if value.len() == 10 && value.get(4) == Some(&b'-') && value.get(7) == Some(&b'-') =>
        {
            (&value[..4], &value[5..7], &value[8..])
        }
        b"YYYYMMDD" if value.len() == 8 => (&value[..4], &value[4..6], &value[6..]),
        _ => return false,
    };
    let number = |bytes: &[u8]| -> Option<u32> {
        bytes.iter().try_fold(0u32, |value, byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(*byte - b'0'))
        })
    };
    let Some(year) = number(year) else {
        return false;
    };
    let Some(month) = number(month) else {
        return false;
    };
    let Some(day) = number(day) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    day >= 1 && day <= days
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
                    source_end_line: 1,
                }],
            })
            .unwrap();
        assert_eq!(output.return_code, 0);
        assert_eq!(output.records, vec![b"BATCH COBOL".to_vec()]);
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
                            source_end_line: 1,
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
                            source_end_line: 9,
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
                    source_end_line: 1,
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
