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
    CardDemoClosureReceipt, CardDemoControlReceipt, CardDemoCoreReceipt, CardDemoCorpusReceipt,
    CardDemoFileCallReceipt, CardDemoHostReceipt, CardDemoLayoutReceipt, CardDemoPackageReceipt,
    CardDemoSourceReceipt, CorpusProblem, verify_carddemo_application_package_from_env,
    verify_carddemo_control_flow_from_env, verify_carddemo_core_semantics_from_env,
    verify_carddemo_corpus, verify_carddemo_corpus_from_env, verify_carddemo_data_layouts_from_env,
    verify_carddemo_file_call_semantics_from_env, verify_carddemo_host_operands_from_env,
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
        assert!(
            compile("IDENTIFICATION DIVISION. PROGRAM-ID. X. PROCEDURE DIVISION. INVOKE X.")
                .is_err()
        );
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
    fn packed_binary_group_condition_and_intrinsic_semantics_execute() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PACKED-X PIC S9(5) COMP-3 VALUE 12. 01 BINARY-X PIC S9(4) COMP VALUE 7. 01 DISPLAY-X PIC 9(5). 01 SOURCE-GROUP. 05 TEXT-X PIC X(3) VALUE 'ABC'. 05 COUNT-X PIC 9(2) VALUE 12. 01 GROUP-OUT PIC X(5). 01 TRIM-X PIC X(8) VALUE ' ab '. 01 TEXT-OUT PIC X(8). 01 FLAG-X PIC X VALUE 'N'. 88 FLAG-YES VALUE 'Y'. PROCEDURE DIVISION. ADD 3 TO PACKED-X. MOVE PACKED-X TO DISPLAY-X. DISPLAY DISPLAY-X. MULTIPLY 3 BY BINARY-X. MOVE BINARY-X TO DISPLAY-X. DISPLAY DISPLAY-X. MOVE SOURCE-GROUP TO GROUP-OUT. DISPLAY GROUP-OUT. MOVE FUNCTION UPPER-CASE(FUNCTION TRIM(TRIM-X)) TO TEXT-OUT. DISPLAY TEXT-OUT. SET FLAG-YES TO TRUE. IF FLAG-YES DISPLAY 'YES' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"00015\n00021\nABC12\nAB      \nYES\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn string_unstring_inspect_initialize_and_figuratives_execute() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. TEXTOPS. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC X(6) VALUE 'AB CD '. 01 B PIC X(3) VALUE 'XYZ'. 01 OUT-X PIC X(10). 01 U-SOURCE PIC X(7) VALUE 'ONE,TWO'. 01 U1 PIC X(3). 01 U2 PIC X(3). 01 TALLY-X PIC 9(2). 01 NUM-X PIC 9(3) VALUE 123. 01 ALPHA-X PIC X(3) VALUE 'ABC'. PROCEDURE DIVISION. STRING A DELIMITED BY SPACE B DELIMITED BY SIZE INTO OUT-X. DISPLAY OUT-X. UNSTRING U-SOURCE DELIMITED BY ',' INTO U1 U2. INSPECT U1 REPLACING ALL 'O' BY 'X'. INSPECT U2 TALLYING TALLY-X FOR ALL 'T'. DISPLAY U1. DISPLAY U2. DISPLAY TALLY-X. INITIALIZE NUM-X ALPHA-X. DISPLAY NUM-X. DISPLAY ALPHA-X. MOVE LOW-VALUES TO ALPHA-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"ABXYZ     \nXNE\nTWO\n01\n000\n   \n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn subscript_reference_modification_and_bounds_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. REFS. DATA DIVISION. WORKING-STORAGE SECTION. 01 TABLE-X PIC X(3) OCCURS 3 TIMES VALUE 'ABC'. 01 TEXT-X PIC X(6) VALUE '123456'. 01 OUT-X PIC X(3). PROCEDURE DIVISION. MOVE TABLE-X(2) TO OUT-X. DISPLAY OUT-X. MOVE 'XYZ' TO TABLE-X(3). MOVE TABLE-X(3) TO OUT-X. DISPLAY OUT-X. MOVE TEXT-X(2:3) TO OUT-X. DISPLAY OUT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"ABC\nXYZ\n234\n")
            }
            other => panic!("{other:?}"),
        }

        let bad = "IDENTIFICATION DIVISION. PROGRAM-ID. BADREF. DATA DIVISION. WORKING-STORAGE SECTION. 01 TABLE-X PIC X OCCURS 2 TIMES. 01 OUT-X PIC X. PROCEDURE DIVISION. MOVE TABLE-X(3) TO OUT-X. STOP RUN.";
        let artifact = compile(bad).unwrap();
        assert!(matches!(execute(&artifact, 1024), MachineDrive::Failed(_)));
    }
    #[test]
    fn decimal_scale_sign_rounding_precedence_and_overflow_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DECIMAL. DATA DIVISION. WORKING-STORAGE SECTION. 01 PACKED-X PIC S9(3)V99 COMP-3 VALUE -12.34. 01 RESULT-X PIC 9V99 COMP-3. 01 DISPLAY-X PIC S9(4)V99. PROCEDURE DIVISION. ADD 2.34 TO PACKED-X. MOVE PACKED-X TO DISPLAY-X. DISPLAY DISPLAY-X. COMPUTE RESULT-X ROUNDED = 1 / 3. MOVE RESULT-X TO DISPLAY-X. DISPLAY DISPLAY-X. COMPUTE RESULT-X ROUNDED = ( 2 + 3 ) * 4 / 3. MOVE RESULT-X TO DISPLAY-X. DISPLAY DISPLAY-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"00100}\n000033\n000667\n")
            }
            other => panic!("{other:?}"),
        }

        let overflow = "IDENTIFICATION DIVISION. PROGRAM-ID. OVERFLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 X PIC 99 VALUE 99. PROCEDURE DIVISION. ADD 1 TO X. STOP RUN.";
        let artifact = compile(overflow).unwrap();
        assert!(matches!(execute(&artifact, 1024), MachineDrive::Failed(_)));
    }
    #[test]
    fn reached_intrinsic_date_case_length_numval_and_mod_are_deterministic() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FUNCTIONS. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATE-X PIC X(21). 01 INTEGER-X PIC 9(7). 01 ROUNDTRIP-X PIC 9(8). 01 MOD-X PIC 9(2). 01 LENGTH-X PIC 9(2). 01 TEXT-X PIC X(8) VALUE ' AbC '. 01 CASE-X PIC X(8). 01 NUMBER-X PIC X(8) VALUE ' 12.50 '. 01 DECIMAL-X PIC 9(3)V99. PROCEDURE DIVISION. MOVE FUNCTION CURRENT-DATE TO DATE-X. DISPLAY DATE-X. MOVE FUNCTION INTEGER-OF-DATE(20240229) TO INTEGER-X. MOVE FUNCTION DATE-OF-INTEGER(INTEGER-X) TO ROUNDTRIP-X. DISPLAY ROUNDTRIP-X. MOVE FUNCTION MOD(17, 5) TO MOD-X. DISPLAY MOD-X. MOVE FUNCTION LENGTH(TEXT-X) TO LENGTH-X. DISPLAY LENGTH-X. MOVE FUNCTION LOWER-CASE(FUNCTION TRIM(TEXT-X)) TO CASE-X. DISPLAY CASE-X. MOVE FUNCTION NUMVAL(NUMBER-X) TO DECIMAL-X. DISPLAY DECIMAL-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(
                done.output.bytes(),
                b"1970010100000000+0000\n20240229\n02\n08\nabc     \n01250\n"
            ),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn multiline_if_evaluate_and_varying_cfg_executes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 N PIC 9 VALUE 1. 01 I PIC 9 VALUE 0. 01 TOTAL PIC 99 VALUE 0. PROCEDURE DIVISION.\nIF N = 1\n DISPLAY 'IF-TRUE'\nELSE\n DISPLAY 'IF-FALSE'\nEND-IF.\nEVALUATE N\n WHEN 1\n  DISPLAY 'EVAL-ONE'\n WHEN OTHER\n  DISPLAY 'EVAL-OTHER'\nEND-EVALUATE.\nPERFORM VARYING I FROM 1 BY 1 UNTIL I > 3\n ADD I TO TOTAL\nEND-PERFORM.\nDISPLAY TOTAL.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"IF-TRUE\nEVAL-ONE\n06\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn structured_loop_checkpoint_preserves_reentry_state() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FLOWCP. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 9. 01 TOTAL PIC 99. PROCEDURE DIVISION.\nPERFORM VARYING I FROM 1 BY 1 UNTIL I > 3\n ADD I TO TOTAL\nEND-PERFORM.\nDISPLAY TOTAL.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        let invocation = invocation(&artifact, 1024);
        let mut first = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        for _ in 0..8 {
            assert_eq!(
                first.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
                MachineDrive::Continue
            );
        }
        let checkpoint = first.checkpoint().unwrap();
        assert_eq!(
            checkpoint.schema(),
            "mainframe-env.reference-machine-checkpoint@2"
        );
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(
            drive_to_terminal(&mut first),
            drive_to_terminal(&mut restored)
        );
    }

    #[test]
    fn call_using_roundtrips_by_reference_storage() {
        use mainframe_env_host_api::{EffectResult, HostRequest, HostResult, ProgramRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CALLER. DATA DIVISION. WORKING-STORAGE SECTION. 01 ARG-X PIC X(2) VALUE 'AB'. PROCEDURE DIVISION. CALL 'SUB' USING ARG-X. DISPLAY ARG-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let effect = loop {
            match machine.drive(MachineResume::Start, Quantum::new(16, 1024).unwrap()) {
                MachineDrive::Continue => {}
                MachineDrive::HostCall(effect) => break effect,
                other => panic!("{other:?}"),
            }
        };
        let HostRequest::Program(ProgramRequest::Call { program, payload }) = &effect.request
        else {
            panic!("unexpected request: {:?}", effect.request);
        };
        assert_eq!(program.as_str(), "SUB");
        assert_eq!(payload.schema(), "mainframe-env.cobol.call@1");
        assert!(payload.bytes().ends_with(b"AB"));
        let mut result = 1u32.to_be_bytes().to_vec();
        result.extend_from_slice(&2u64.to_be_bytes());
        result.extend_from_slice(b"XY");
        let result = mainframe_env_execution_api::BoundedPayload::new(
            "mainframe-env.cobol.call-result@1",
            result,
            InvocationLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Program(result)),
                }),
                Quantum::new(32, 1024).unwrap(),
            ),
            MachineDrive::Completed(done) if done.output.bytes() == b"XY\n"
        ));
    }

    #[test]
    fn dataset_read_uses_dd_binding_and_updates_file_status() {
        use mainframe_env_host_api::{
            DatasetRequest, DatasetResult, EffectResult, HostRequest, HostResult,
        };

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. READER. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X(3). 01 STATUS-X PIC XX. PROCEDURE DIVISION. READ INPUT-FILE INTO REC-X. DISPLAY REC-X. DISPLAY STATUS-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cobol.dd.INPUT-FILE".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.dataset-name@1",
                b"USER.INPUT".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        invocation.bindings.insert(
            "cobol.file-status.INPUT-FILE".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.storage-name@1",
                b"STATUS-X".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap())
        else {
            panic!("dataset read did not call host");
        };
        assert!(matches!(
            effect.request,
            HostRequest::Dataset(DatasetRequest::Read { ref dataset, .. })
                if dataset.as_str() == "USER.INPUT"
        ));
        assert!(matches!(
            machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Dataset(DatasetResult::Records {
                        records: vec![b"ABC".to_vec()],
                        version: 1,
                    })),
                }),
                Quantum::new(64, 1024).unwrap(),
            ),
            MachineDrive::Completed(done) if done.output.bytes() == b"ABC\n00\n"
        ));
    }

    #[test]
    fn cics_outbound_operands_read_storage_bytes() {
        use mainframe_env_host_api::{CicsOperation, HostRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(3) VALUE 'ABC'. PROCEDURE DIVISION. EXEC CICS WRITEQ TD QUEUE('Q1') FROM(DATA-X) LENGTH(3) END-EXEC. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap())
        else {
            panic!("CICS operation did not call host");
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("unexpected host request");
        };
        assert_eq!(request.operation, CicsOperation::WriteTransientData);
        assert_eq!(request.arguments["FROM"].bytes(), b"ABC");
        assert_eq!(request.arguments["QUEUE"].bytes(), b"Q1");
        assert_eq!(request.arguments["LENGTH"].bytes(), b"3");
    }

    #[test]
    fn sql_host_variables_use_typed_embedded_envelope() {
        use mainframe_env_host_api::{EffectResult, HostRequest, HostResult, ProgramRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SQLABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 IN-X PIC X(2) VALUE '42'. 01 OUT-X PIC X(3). PROCEDURE DIVISION. EXEC SQL SELECT NAME INTO :OUT-X FROM CUSTOMER WHERE ID = :IN-X END-EXEC. DISPLAY OUT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap())
        else {
            panic!("SQL operation did not call host");
        };
        let HostRequest::Program(ProgramRequest::Call { program, payload }) = &effect.request
        else {
            panic!("unexpected SQL request");
        };
        assert_eq!(program.as_str(), "MAINFRAME-SQL");
        assert_eq!(payload.schema(), "mainframe-env.embedded-host@1");
        assert!(payload.bytes().starts_with(b"MEHOST01"));
        assert!(payload.bytes().windows(2).any(|window| window == b"42"));
        assert!(
            !payload
                .bytes()
                .windows(8)
                .any(|window| window == b"CUSTOMER")
        );
        let mut result = 1u32.to_be_bytes().to_vec();
        result.extend_from_slice(&3u64.to_be_bytes());
        result.extend_from_slice(b"ANN");
        let result = mainframe_env_execution_api::BoundedPayload::new(
            "mainframe-env.cobol.call-result@1",
            result,
            InvocationLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Program(result)),
                }),
                Quantum::new(64, 1024).unwrap(),
            ),
            MachineDrive::Completed(done) if done.output.bytes() == b"ANN\n"
        ));
    }

    #[test]
    fn dli_pcb_and_ssa_are_typed_read_write_operands() {
        use mainframe_env_host_api::{HostRequest, ProgramRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DLIABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 PCB-X PIC X(4) VALUE 'PCB1'. 01 SSA-X PIC X(4) VALUE 'SSA1'. PROCEDURE DIVISION. EXEC DLI GU USING PCB-X SSA-X END-EXEC. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap())
        else {
            panic!("DLI operation did not call host");
        };
        let HostRequest::Program(ProgramRequest::Call { program, payload }) = effect.request else {
            panic!("unexpected DLI request");
        };
        assert_eq!(program.as_str(), "MAINFRAME-DLI");
        assert_eq!(payload.schema(), "mainframe-env.embedded-host@1");
        assert!(payload.bytes().windows(4).any(|window| window == b"PCB1"));
        assert!(payload.bytes().windows(4).any(|window| window == b"SSA1"));
    }

    #[test]
    fn mq_call_parameter_list_carries_names_modes_and_values() {
        use mainframe_env_host_api::{HostRequest, ProgramRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MQABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 HCONN PIC X(4) VALUE 'HC01'. 01 HOBJ PIC X(4) VALUE 'HO01'. 01 CC PIC 9(4) VALUE 0. 01 RC PIC 9(4) VALUE 0. PROCEDURE DIVISION. CALL 'MQOPEN' USING HCONN HOBJ CC RC. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap())
        else {
            panic!("MQ call did not call host");
        };
        let HostRequest::Program(ProgramRequest::Call { program, payload }) = effect.request else {
            panic!("unexpected MQ request");
        };
        assert_eq!(program.as_str(), "MQOPEN");
        assert_eq!(payload.schema(), "mainframe-env.cobol.call@1");
        for expected in [b"HCONN".as_slice(), b"HOBJ", b"CC", b"RC", b"HC01", b"HO01"] {
            assert!(
                payload
                    .bytes()
                    .windows(expected.len())
                    .any(|window| window == expected)
            );
        }
    }

    fn drive_to_terminal(
        machine: &mut ReferenceMachine,
    ) -> MachineDrive<mainframe_env_host_api::EffectRequest> {
        loop {
            match machine.drive(MachineResume::Start, Quantum::new(16, 1024).unwrap()) {
                MachineDrive::Continue => {}
                terminal => return terminal,
            }
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
