use mainframe_env_batch::{Program, ProgramInput, ProgramOutput, ProgramRouter};
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, Invocation, InvocationLimits, Machine, MachineDrive,
    MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_host_api::{HostProblem, HostProvider};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub fn default_program_router() -> Arc<dyn HostProvider> {
    ProgramRouter::with_builtins_and(
        BTreeMap::from([("COBOL".into(), Arc::new(CobolProgram) as Arc<dyn Program>)]),
        InvocationLimits::default(),
    )
    .expect("owned program catalog is valid")
}

struct CobolProgram;

impl Program for CobolProgram {
    fn execute(&self, input: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
        let source = input
            .dds
            .iter()
            .find(|dd| dd.name == "SYSIN")
            .map(|dd| dd.inline_data.clone())
            .ok_or(HostProblem::NotFound)?;
        let source_limits = SourceLimits::default();
        let path = LogicalPath::new("SYSIN.cbl", source_limits.max_path_bytes)
            .map_err(|_| HostProblem::Malformed)?;
        let file = SourceFile::input(
            "SYSIN.cbl",
            source,
            SourceFormat::Free,
            SourceEncoding::Utf8,
            source_limits,
        )
        .map_err(|_| HostProblem::Malformed)?;
        let source = SourceBundle::new(
            &path,
            vec![file],
            BTreeMap::new(),
            Vec::new(),
            source_limits,
        )
        .map_err(|_| HostProblem::Malformed)?;
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
        let invocation = Invocation::new(
            RequestId::new("batch-cobol-request", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new("batch-cobol-execution", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            RunUnitId::new("batch-cobol-run", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            None,
            Selector::new("program:COBOL", limits).map_err(|_| HostProblem::Malformed)?,
            ArtifactRef::new(format!("sha256:{}", artifact.id().to_hex()), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            Principal::new(
                PrincipalId::new("BATCH", limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                BTreeSet::new(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            ServiceClass::Batch,
            0,
            1000,
            TraceId::new("batch-cobol-trace", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new("batch-cobol-effect", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .map_err(|_| HostProblem::ProviderFailure)?;
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(
                resume,
                Quantum::new(10_000, 16 * 1024 * 1024).expect("non-zero quantum"),
            ) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::Completed(completion) => {
                    return Ok(ProgramOutput {
                        return_code: completion.return_code,
                        records: completion
                            .output
                            .bytes()
                            .split(|byte| *byte == b'\n')
                            .filter(|record| !record.is_empty())
                            .map(<[u8]>::to_vec)
                            .collect(),
                    });
                }
                MachineDrive::HostCall(_) => return Err(HostProblem::ProviderFailure),
                MachineDrive::Condition(condition) => {
                    return Ok(ProgramOutput {
                        return_code: condition.response,
                        records: vec![condition.name.into_bytes()],
                    });
                }
                MachineDrive::Abend(_) => {
                    return Err(HostProblem::Condition {
                        name: "ABEND".into(),
                        response: -1,
                        response2: 0,
                    });
                }
                MachineDrive::Failed(_) => return Err(HostProblem::ProviderFailure),
                MachineDrive::Suspended(_)
                | MachineDrive::Invoke(_)
                | MachineDrive::Transfer(_) => return Err(HostProblem::Unsupported),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_batch::DdPlan;

    #[test]
    fn default_cobol_program_compiles_and_runs_reference_machine() {
        let program = CobolProgram;
        let output = program
            .execute(&ProgramInput {
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
}
