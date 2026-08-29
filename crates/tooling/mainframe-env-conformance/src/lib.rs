//! Independent selected-route fixtures for mainframe-env 0.1.

#![forbid(unsafe_code)]

use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService, PublishedArtifact,
};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, Invocation, InvocationLimits, Machine, MachineDrive,
    MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};

mod carddemo;

pub use carddemo::{
    CardDemoClosureReceipt, CardDemoControlReceipt, CardDemoCorpusReceipt, CardDemoLayoutReceipt,
    CardDemoSourceReceipt, CorpusProblem, verify_carddemo_control_flow_from_env,
    verify_carddemo_corpus, verify_carddemo_corpus_from_env, verify_carddemo_data_layouts_from_env,
    verify_carddemo_source_closures_from_env, verify_carddemo_source_preprocessing_from_env,
};

pub const HELLO_SOURCE: &str = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 MSG PIC X(12) VALUE 'HELLO WORLD!'.\nPROCEDURE DIVISION.\nDISPLAY MSG.\nSTOP RUN.\n";

pub fn source_bundle(source: &str) -> SourceBundle {
    source_bundle_with_format(source, SourceFormat::Free)
}

pub fn source_bundle_with_format(source: &str, format: SourceFormat) -> SourceBundle {
    let limits = SourceLimits::default();
    let path = LogicalPath::new("HELLO.cbl", limits.max_path_bytes).expect("fixture path");
    let file = SourceFile::input(
        "HELLO.cbl",
        source.as_bytes().to_vec(),
        format,
        SourceEncoding::Utf8,
        limits,
    )
    .expect("fixture source");
    SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits)
        .expect("fixture bundle")
}
pub fn compile(source: &str) -> Result<PublishedArtifact, String> {
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source: source_bundle(source),
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").map_err(|e| e.to_string())?,
            options: CompileOptions::new(BTreeMap::new()).map_err(|e| e.to_string())?,
        })
        .map_err(|e| e.to_string())?;
    match result {
        CompilerResult::Published { artifact, .. } => Ok(artifact),
        other => Err(format!("not published: {other:?}")),
    }
}
pub fn invocation(artifact: &PublishedArtifact, output_limit: u64) -> Invocation {
    let l = InvocationLimits::default();
    let resource = ResourceLimits {
        max_output_bytes: output_limit,
        ..ResourceLimits::default()
    };
    Invocation::new(
        RequestId::new("fixture-request", l).unwrap(),
        ExecutionId::new("fixture-execution", l).unwrap(),
        RunUnitId::new("fixture-run", l).unwrap(),
        None,
        Selector::new("program:COBOL:HELLO", l).unwrap(),
        ArtifactRef::new(format!("sha256:{}", artifact.id().to_hex()), l).unwrap(),
        Principal::new(PrincipalId::new("IBMUSER", l).unwrap(), BTreeSet::new(), l).unwrap(),
        ServiceClass::Batch,
        0,
        1000,
        TraceId::new("fixture-trace", l).unwrap(),
        IdempotencyKey::new("fixture-idempotency", l).unwrap(),
        1,
        resource,
        BTreeMap::new(),
        l,
    )
    .unwrap()
}
pub fn execute(
    artifact: &PublishedArtifact,
    output_limit: u64,
) -> MachineDrive<mainframe_env_host_api::EffectRequest> {
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation(artifact, output_limit),
        CodecLimits::default(),
    )
    .expect("legal fixture artifact");
    let mut resume = MachineResume::Start;
    loop {
        match machine.drive(resume, Quantum::new(8, 4096).unwrap()) {
            MachineDrive::Continue => resume = MachineResume::Start,
            terminal => return terminal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_diagnostics::Completeness;
    use mainframe_env_encoding::CodePage;
    use proptest::prelude::*;
    #[test]
    fn hello_selected_route_matches_frozen_output() {
        let artifact = compile(HELLO_SOURCE).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.return_code, 0);
                assert_eq!(done.output.bytes(), b"HELLO WORLD!\n");
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn openmainframe_hello_fixture_matches_exact_oracle_output() {
        let source = include_str!("../../../../conformance/0.1/fixtures/cobol/HELLO.cbl");
        let result = CobolCompiler::default()
            .compile(CompilerRequest {
                source: source_bundle_with_format(source, SourceFormat::Fixed),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("fixture did not publish: {result:?}");
        };
        match execute(&artifact, 4096) {
            MachineDrive::Completed(done) => assert_eq!(
                done.output.bytes(),
                b"================================\n     zOS-clone Hello World\n================================\nHello, World!       \nGoodbye!\n"
            ),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn deterministic_replay_is_byte_exact() {
        let artifact = compile(HELLO_SOURCE).unwrap();
        assert_eq!(execute(&artifact, 1024), execute(&artifact, 1024));
    }
    #[test]
    fn malformed_source_fails_without_artifact() {
        let source = "IDENTIFICATION DIVISION. PROCEDURE DIVISION. BOGUS THING.";
        let result = CobolCompiler::default()
            .compile(CompilerRequest {
                source: source_bundle(source),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        assert!(matches!(
            result,
            CompilerResult::Failed {
                completeness: Completeness::Incomplete,
                ..
            }
        ));
    }
    #[test]
    fn unsupported_source_fails_without_artifact() {
        assert!(compile("IDENTIFICATION DIVISION. PROGRAM-ID. X. PROCEDURE DIVISION. EXEC SQL SELECT 1 END-EXEC.").is_err());
    }
    #[test]
    fn output_exhaustion_is_typed_failure() {
        let artifact = compile(HELLO_SOURCE).unwrap();
        assert!(matches!(execute(&artifact, 1), MachineDrive::Failed(_)));
    }
    #[test]
    fn compiler_and_interpreter_operation_registries_match() {
        let compiler: BTreeSet<_> = mainframe_env_compiler::core_mir_catalog()
            .identities()
            .cloned()
            .collect();
        assert_eq!(
            compiler,
            mainframe_env_interpreter::supported_operations().clone()
        );
    }
    #[test]
    fn every_frozen_supported_statement_has_an_executable_route() {
        let cases = [
            "ACCEPT A",
            "ADD A TO B",
            "ALLOCATE A RETURNING P",
            "ALTER PARA TO PROCEED TO PARA2",
            "CALL 'SUB'",
            "CANCEL 'SUB'",
            "CLOSE FILE1",
            "COMPUTE B = A + 1",
            "CONTINUE",
            "DISPLAY A",
            "DIVIDE A INTO B",
            "ENTRY 'ALT'",
            "EVALUATE A WHEN 1 DISPLAY 'ONE' END-EVALUATE",
            "EXEC CICS RETURN END-EXEC",
            "EXIT",
            "FREE P",
            "GOBACK",
            "GO TO PARA",
            "IF A = 1 DISPLAY 'YES' END-IF",
            "INITIALIZE A",
            "INSPECT A REPLACING ALL 'A' BY 'B'",
            "JSON GENERATE J FROM A",
            "JSON PARSE J INTO A",
            "MOVE A TO B",
            "MULTIPLY A BY B",
            "OPEN INPUT FILE1",
            "PERFORM PARA",
            "READ FILE1 INTO A",
            "SEARCH T WHEN A = B DISPLAY 'X' END-SEARCH",
            "SET A TO 1",
            "STOP RUN",
            "STRING A DELIMITED BY SIZE INTO B",
            "SUBTRACT A FROM B",
            "UNSTRING A DELIMITED BY SPACE INTO B",
            "WRITE FILE1 A",
            "XML GENERATE J FROM A",
            "XML PARSE J INTO A",
        ];
        assert_eq!(cases.len(), 37);
        for statement in cases {
            let source = format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. ROUTE. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9(3) VALUE 1. 01 B PIC 9(3) VALUE 2. 01 P USAGE POINTER. 01 J PIC X(64). 01 T PIC X(8). PROCEDURE DIVISION. {statement}. STOP RUN. PARA. EXIT."
            );
            assert!(compile(&source).is_ok(), "missing route for {statement}");
        }
    }
    #[test]
    fn internal_data_arithmetic_and_condition_semantics_execute() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MATH. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9(3) VALUE 2. 01 B PIC 9(3) VALUE 3. 01 OUT PIC X(5). PROCEDURE DIVISION. ADD A TO B. IF B = 5 DISPLAY 'OK' END-IF. MOVE 'DONE' TO OUT. DISPLAY B. DISPLAY OUT. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n005\nDONE \n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn declared_cp037_source_compiles_and_executes() {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("HELLO.cbl", limits.max_path_bytes).unwrap();
        let bytes = CodePage::Cp037.encode(HELLO_SOURCE, 64 * 1024).unwrap();
        let file = SourceFile::input(
            "HELLO.cbl",
            bytes,
            SourceFormat::Free,
            SourceEncoding::Ebcdic(37),
            limits,
        )
        .unwrap();
        let source =
            SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
        let result = CobolCompiler::default()
            .compile(CompilerRequest {
                source,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("CP037 source did not publish: {result:?}");
        };
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Completed(_)
        ));
    }
    #[test]
    fn copy_replacing_expands_from_exact_dependency_bytes() {
        let primary = "IDENTIFICATION DIVISION. PROGRAM-ID. COPYTEST. DATA DIVISION. WORKING-STORAGE SECTION. COPY REC REPLACING ==TOKEN== BY ==HELLO==. PROCEDURE DIVISION. DISPLAY FIELD. STOP RUN.";
        let copy = "01 FIELD PIC X(5) VALUE 'TOKEN'.";
        let limits = SourceLimits::default();
        let path = LogicalPath::new("COPYTEST.cbl", limits.max_path_bytes).unwrap();
        let files = vec![
            SourceFile::input(
                "COPYTEST.cbl",
                primary.as_bytes().to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
            SourceFile::input(
                "copy/REC.cpy",
                copy.as_bytes().to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ];
        let bundle = SourceBundle::new(&path, files, BTreeMap::new(), Vec::new(), limits).unwrap();
        let analysis = CobolCompiler::default().analyze(&bundle);
        assert_eq!(analysis.syntax.as_ref().unwrap().expansions().len(), 1);
        assert_eq!(
            analysis
                .semantic
                .as_ref()
                .unwrap()
                .layout("FIELD")
                .unwrap()
                .initial,
            b"HELLO"
        );
        let result = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap();
        let CompilerResult::Published { artifact, .. } = result else {
            panic!("COPY program did not publish: {result:?}");
        };
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"HELLO\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn cancellation_timeout_and_quantum_bound_are_explicit() {
        let artifact = compile(HELLO_SOURCE).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            machine.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Continue
        ));
        assert!(matches!(
            machine.drive(MachineResume::Cancelled, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Failed(problem) if problem.category == mainframe_env_diagnostics::FailureCategory::Cancelled
        ));
        let mut timed = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            timed.drive(MachineResume::TimedOut, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Failed(problem) if problem.category == mainframe_env_diagnostics::FailureCategory::TimedOut
        ));
    }
    #[test]
    fn provider_failure_is_not_normal_completion() {
        let artifact = compile("IDENTIFICATION DIVISION. PROGRAM-ID. INPUT. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC X(8). PROCEDURE DIVISION. ACCEPT A. STOP RUN.").unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap())
        else {
            panic!("ACCEPT did not issue a typed host call");
        };
        let failed = machine.drive(
            MachineResume::HostResult(mainframe_env_host_api::EffectResult {
                sequence: effect.sequence,
                outcome: Err(mainframe_env_host_api::HostProblem::ProviderFailure),
            }),
            Quantum::new(100, 1024).unwrap(),
        );
        assert!(matches!(
            failed,
            MachineDrive::Failed(problem) if problem.category == mainframe_env_diagnostics::FailureCategory::ProviderFailure
        ));
    }
    proptest! {#[test]fn parser_never_panics_on_bounded_text(input in ".{0,2048}"){let _=CobolCompiler::default().analyze(&source_bundle(&format!("IDENTIFICATION DIVISION. PROGRAM-ID. P. PROCEDURE DIVISION. {input}")));}}
}
