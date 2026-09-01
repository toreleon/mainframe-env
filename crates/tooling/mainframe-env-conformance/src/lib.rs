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

mod abi;
mod carddemo;
mod cobol_clauses;
mod cobol_frontend;
mod cobol_functions;
mod cobol_statements;

pub use abi::{
    HostAbiInventoryReceipt, HostAbiLibraryReceipt, HostAbiMemberReceipt, verify_host_abi_libraries,
};

pub use carddemo::{
    CardDemoBaseBatchReceipt, CardDemoBaseOnlineReceipt, CardDemoBatchProgramReceipt,
    CardDemoCicsReceipt, CardDemoCicsRuntimeReceipt, CardDemoClosureReceipt,
    CardDemoControlReceipt, CardDemoCoreReceipt, CardDemoCorpusReceipt,
    CardDemoDatasetCatalogReceipt, CardDemoDb2Receipt, CardDemoFileCallReceipt,
    CardDemoFullReceipt, CardDemoHostReceipt, CardDemoImsReceipt, CardDemoJclReceipt,
    CardDemoLayoutReceipt, CardDemoMqAuthorizationReceipt, CardDemoPackageReceipt,
    CardDemoProgramReceipt, CardDemoResourceReceipt, CardDemoSecurityReceipt, CardDemoSeedReceipt,
    CardDemoSourceReceipt, CardDemoTerminalReceipt, CardDemoUtilityReceipt, CardDemoVsamReceipt,
    CorpusProblem, verify_carddemo_application_package_from_env,
    verify_carddemo_base_batch_from_env, verify_carddemo_base_online_from_env,
    verify_carddemo_batch_programs_from_env, verify_carddemo_cics_abi_from_env,
    verify_carddemo_cics_runtime_from_env, verify_carddemo_control_flow_from_env,
    verify_carddemo_core_semantics_from_env, verify_carddemo_corpus,
    verify_carddemo_corpus_from_env, verify_carddemo_data_layouts_from_env,
    verify_carddemo_dataset_catalog_from_env, verify_carddemo_db2_from_env,
    verify_carddemo_file_call_semantics_from_env, verify_carddemo_full_from_env,
    verify_carddemo_host_operands_from_env, verify_carddemo_ims_from_env,
    verify_carddemo_jcl_from_env, verify_carddemo_mq_authorization_from_env,
    verify_carddemo_program_routing_from_env, verify_carddemo_resources_from_env,
    verify_carddemo_security_from_env, verify_carddemo_seeds_from_env,
    verify_carddemo_source_closures_from_env, verify_carddemo_source_preprocessing_from_env,
    verify_carddemo_terminal_from_env, verify_carddemo_utilities_from_env,
    verify_carddemo_vsam_from_env,
};

pub use cobol_clauses::verify_cobol_semantic_fixtures;
pub use cobol_frontend::{cobol_frontend_runtime, verify_cobol_frontend_fixtures};
pub use cobol_functions::verify_cobol_function_fixtures;
pub use cobol_statements::verify_cobol_statement_fixtures;

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
    fn binary_parent_condition_name_uses_numeric_value() {
        let artifact = compile(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. CONDITION.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 RESULT PIC S9(9) COMP.\n 88 RESULT-OK VALUE 0.\nPROCEDURE DIVISION.\nMOVE 0 TO RESULT.\nIF RESULT-OK\n DISPLAY 'OK'\nELSE\n DISPLAY 'BAD'\nEND-IF.\nSTOP RUN.\n",
        )
        .unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn add_to_zero_giving_writes_the_giving_target() {
        let artifact = compile(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. ADDGIVE.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 RESULT PIC S9(9) COMP.\nPROCEDURE DIVISION.\nADD 8 TO ZERO GIVING RESULT.\nIF RESULT = 8 DISPLAY 'OK' ELSE DISPLAY 'BAD' END-IF.\nSTOP RUN.\n",
        )
        .unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn set_address_of_has_a_bounded_virtual_pointer_route() {
        let artifact = compile(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. POINTERS.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 PTR POINTER.\nLINKAGE SECTION.\n01 BLOCK PIC X.\nPROCEDURE DIVISION.\nSET ADDRESS OF BLOCK TO PTR.\nDISPLAY 'OK'.\nSTOP RUN.\n",
        )
        .unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn nested_occurs_accepts_ordered_multidimensional_subscripts() {
        let artifact = compile(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. MULTIDIM.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TABLE-A.\n 05 ROW-A OCCURS 2 TIMES.\n  10 CELL-A OCCURS 3 TIMES PIC X.\nPROCEDURE DIVISION.\nMOVE 'Z' TO CELL-A(2 3).\nIF CELL-A(2 3) = 'Z' DISPLAY 'OK' ELSE DISPLAY 'BAD' END-IF.\nSTOP RUN.\n",
        )
        .unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn alter_redirects_dynamic_go_to_before_static_control_edges() {
        let artifact = compile(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. ALTERGO.\nPROCEDURE DIVISION.\nALTER DISPATCH TO PROCEED TO SECOND-PARA.\nGO TO DISPATCH.\nDISPATCH.\nGO TO FIRST-PARA.\nFIRST-PARA.\nDISPLAY 'BAD'.\nSTOP RUN.\nSECOND-PARA.\nDISPLAY 'OK'.\nSTOP RUN.\n",
        )
        .unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
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
    fn inspect_converting_translates_reference_modified_receivers() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CONVERT. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X(5) VALUE 'AB1CD'. 01 FROM-X PIC X(4) VALUE 'ABCD'. 01 TO-X PIC X(4) VALUE SPACES. PROCEDURE DIVISION. INSPECT TEXT-X(1:4) CONVERTING FROM-X TO TO-X. DISPLAY TEXT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"  1 D\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn initialize_resolves_multiple_qualified_targets() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INITQUAL. DATA DIVISION. WORKING-STORAGE SECTION. 01 GROUP-X. 05 FIRST-X PIC X VALUE 'A'. 05 SECOND-X PIC X VALUE 'B'. 01 THIRD-X PIC X VALUE 'C'. PROCEDURE DIVISION. INITIALIZE FIRST-X OF GROUP-X SECOND-X OF GROUP-X THIRD-X. DISPLAY GROUP-X. DISPLAY THIRD-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"  \n \n"),
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
    fn justified_right_move_pads_on_the_left() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. JUSTIFY. DATA DIVISION. WORKING-STORAGE SECTION. 01 SOURCE-X PIC X(2) VALUE '1 '. 01 TARGET-X PIC X(2) JUST RIGHT. PROCEDURE DIVISION. MOVE SOURCE-X(1:1) TO TARGET-X. INSPECT TARGET-X REPLACING ALL ' ' BY '0'. DISPLAY TARGET-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"01\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn reference_modification_accepts_length_expressions() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. REFMOD. DATA DIVISION. WORKING-STORAGE SECTION. 01 CARDDEMO-COMMAREA PIC X(3). 01 WS-THIS-PROGCOMMAREA PIC X(2) VALUE 'XY'. 01 WS-COMMAREA PIC X(5). PROCEDURE DIVISION. MOVE WS-THIS-PROGCOMMAREA TO WS-COMMAREA(LENGTH OF CARDDEMO-COMMAREA + 1: LENGTH OF WS-THIS-PROGCOMMAREA). DISPLAY WS-COMMAREA. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"   XY\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn zero_length_reference_modification_is_a_noop() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. ZLEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 SOURCE-X PIC X(5) VALUE SPACES. 01 TARGET-X PIC X(5) VALUE 'ABCDE'. 01 LENGTH-X PIC 9 VALUE 0. PROCEDURE DIVISION. MOVE SOURCE-X(1:LENGTH-X) TO TARGET-X(3:LENGTH-X). DISPLAY TARGET-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"ABCDE\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn subscripted_condition_names_resolve_their_occurring_parent() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CONDITION. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLAGS PIC X(2) VALUE ' S'. 01 FLAG-ARRAY REDEFINES FLAGS. 05 FLAG PIC X OCCURS 2 TIMES. 88 SELECTED VALUE 'S'. 01 I PIC 9 VALUE 2. 01 ZERO-I PIC 9 VALUE 0. 01 PROTECT PIC X VALUE '1'. 88 PROTECT-YES VALUE '1'. 01 DONE-X PIC X VALUE '0'. 88 DONE-YES VALUE '1'. 01 ERROR-X PIC X VALUE '1'. 88 ERROR-ON VALUE '1'. 01 VALUE-X PIC X VALUE 'A'. PROCEDURE DIVISION. IF SELECTED(ZERO-I) DISPLAY 'NO' END-IF. IF SELECTED(I) DISPLAY 'YES' END-IF. IF VALUE-X = LOW-VALUES OR PROTECT-YES DISPLAY 'OR' END-IF. IF PROTECT = '1' DISPLAY 'LITERAL' END-IF. IF I >= 3 OR DONE-YES OR ERROR-ON DISPLAY 'CHAIN' END-IF. IF VALUE-X = 'A' AND NOT DONE-YES DISPLAY 'AND-NOT' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"YES\nOR\nLITERAL\nCHAIN\nAND-NOT\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn reached_dfhresp_constants_are_numeric_evaluate_subjects() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. RESPONSE. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP PIC 9 VALUE 0. PROCEDURE DIVISION.\nEVALUATE RESP\n WHEN DFHRESP(NORMAL)\n  DISPLAY 'OK'\n WHEN OTHER\n  DISPLAY 'NO'\nEND-EVALUATE.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn grouped_evaluate_when_values_share_the_following_body() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. GROUPWHEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 VALUE-X PIC X VALUE 'Y'. PROCEDURE DIVISION.\nEVALUATE VALUE-X\n WHEN 'Y'\n WHEN 'y'\n  DISPLAY 'YES'\n WHEN OTHER\n  DISPLAY 'NO'\nEND-EVALUATE.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"YES\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn evaluate_executes_only_the_first_matching_when_body() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. EVALONCE. DATA DIVISION. WORKING-STORAGE SECTION. 01 VALUE-X PIC X VALUE 'A'. PROCEDURE DIVISION.\nEVALUATE TRUE\n WHEN VALUE-X = 'A'\n  MOVE 'B' TO VALUE-X\n  IF VALUE-X = 'B'\n   CONTINUE\n  END-IF\n WHEN VALUE-X = 'B'\n  DISPLAY 'WRONG'\n WHEN OTHER\n  DISPLAY 'ALSO-WRONG'\nEND-EVALUATE.\nDISPLAY VALUE-X.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"B\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn evaluate_true_compares_binary_sqlcode_to_zero() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SQLZERO. DATA DIVISION. WORKING-STORAGE SECTION. 01 SQLCODE PIC S9(9) COMP-5 VALUE 0. PROCEDURE DIVISION.\nEVALUATE TRUE\n WHEN SQLCODE = ZERO\n  DISPLAY 'OK'\n WHEN OTHER\n  DISPLAY 'NO'\nEND-EVALUATE.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn floating_minus_picture_formats_reached_sqlcodes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SQLFORMAT. DATA DIVISION. WORKING-STORAGE SECTION. 01 SQLCODE PIC S9(9) COMP-5 VALUE 100. 01 WS-DISP-SQLCODE PIC ----9. PROCEDURE DIVISION. MOVE SQLCODE TO WS-DISP-SQLCODE. DISPLAY WS-DISP-SQLCODE. MOVE -911 TO SQLCODE. MOVE SQLCODE TO WS-DISP-SQLCODE. DISPLAY WS-DISP-SQLCODE. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"  100\n -911\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn explicit_then_is_not_part_of_the_if_condition() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. IFTHEN. DATA DIVISION. WORKING-STORAGE SECTION. 01 FILE-STATUS PIC XX VALUE '00'. PROCEDURE DIVISION. IF FILE-STATUS = '00' THEN DISPLAY 'OK' ELSE DISPLAY 'NO' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn perform_through_same_paragraph_returns_at_its_sentence_endpoint() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SAMETHRU. PROCEDURE DIVISION. PERFORM WORK-PARA THRU WORK-PARA. DISPLAY 'DONE'. STOP RUN. WORK-PARA. DISPLAY 'WORK'. WORK-EXIT. EXIT. NEXT-PARA. DISPLAY 'WRONG'.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"WORK\nDONE\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn nested_performs_preserve_the_outer_same_paragraph_endpoint() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. NESTTHRU. PROCEDURE DIVISION. PERFORM OUTER-PARA THRU OUTER-PARA. DISPLAY 'DONE'. STOP RUN. OUTER-PARA. DISPLAY 'OUTER'. PERFORM INNER-PARA THRU INNER-EXIT. OUTER-EXIT. EXIT. INNER-PARA. DISPLAY 'INNER'. INNER-EXIT. EXIT. NEXT-PARA. DISPLAY 'WRONG'.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"OUTER\nINNER\nDONE\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn inline_perform_loop_resumes_after_nested_paragraph_perform() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. NESTLOOP. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 9 VALUE 0. 01 DONE-X PIC X VALUE 'N'. PROCEDURE DIVISION.\nPERFORM READ-PARA\nPERFORM UNTIL DONE-X = 'Y'\n PERFORM TREAT-PARA\n PERFORM READ-PARA\nEND-PERFORM.\nSTOP RUN.\nREAD-PARA.\nADD 1 TO I.\nIF I > 3\n MOVE 'Y' TO DONE-X\nEND-IF.\nEXIT.\nTREAT-PARA.\nDISPLAY I.\nEXIT.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"1\n2\n3\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn nested_read_perform_takes_at_end_branch_before_loop_reentry() {
        use mainframe_env_host_api::{DatasetResult, EffectResult, HostRequest, HostResult};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. READLOOP. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC-X PIC X. 01 LASTREC PIC X VALUE 'N'. PROCEDURE DIVISION.\nPERFORM READ-PARA\nPERFORM UNTIL LASTREC = 'Y'\n DISPLAY REC-X\n PERFORM READ-PARA\nEND-PERFORM.\nSTOP RUN.\nREAD-PARA.\nREAD INPUT-FILE INTO REC-X\n AT END MOVE 'Y' TO LASTREC\nEND-READ.\nEXIT.";
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
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let mut records = [b"A".to_vec(), b"B".to_vec(), b"C".to_vec()]
            .into_iter()
            .map(|record| vec![record])
            .chain(std::iter::once(Vec::new()));
        let mut resume = MachineResume::Start;
        let mut reads = 0usize;
        for _ in 0..100 {
            match machine.drive(resume, Quantum::new(256, 1024).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    assert!(matches!(effect.request, HostRequest::Dataset(_)));
                    reads += 1;
                    let records = records.next().expect("bounded read count");
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: Ok(HostResult::Dataset(DatasetResult::Records {
                            identities: records.clone(),
                            records,
                            version: reads as u64,
                        })),
                    });
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(reads, 4);
                    assert_eq!(done.output.bytes(), b"A\nB\nC\n");
                    return;
                }
                other => panic!(
                    "{other:?}; reads={reads}; position={}",
                    machine.position_summary()
                ),
            }
        }
        panic!("read loop did not terminate");
    }
    #[test]
    fn split_relational_operators_are_normalized_in_conditions() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. RELATION. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 9 VALUE 2. PROCEDURE DIVISION. IF I >= 2 DISPLAY 'YES' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"YES\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn numeric_display_input_can_be_compared_to_a_nonnumeric_sentinel() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SENTINEL. DATA DIVISION. WORKING-STORAGE SECTION. 01 RAW-X PIC X VALUE '*'. 01 INPUT-N REDEFINES RAW-X PIC 9. 01 VALID-N PIC 9 VALUE 1. PROCEDURE DIVISION. IF INPUT-N = '*' DISPLAY 'SENTINEL' END-IF. IF VALID-N = '*' DISPLAY 'NO' END-IF. IF INPUT-N NOT NUMERIC DISPLAY 'CLASS' END-IF. IF VALID-N NOT NUMERIC DISPLAY 'NO' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"SENTINEL\nCLASS\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn numeric_display_move_to_alphanumeric_preserves_leading_zeroes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. NUMMOVE. DATA DIVISION. WORKING-STORAGE SECTION. 01 NUMBER-X PIC 9(3) VALUE 5. 01 TEXT-X PIC X(3). PROCEDURE DIVISION. MOVE NUMBER-X TO TEXT-X. DISPLAY TEXT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"005\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn low_value_sentinel_moved_to_numeric_display_increments_from_zero() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. LOWKEY. DATA DIVISION. WORKING-STORAGE SECTION. 01 TRAN-ID PIC X(16) VALUE LOW-VALUES. 01 TRAN-ID-N PIC 9(16). PROCEDURE DIVISION. MOVE TRAN-ID TO TRAN-ID-N. ADD 1 TO TRAN-ID-N. DISPLAY TRAN-ID-N. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"0000000000000001\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn figurative_moves_fill_receivers_and_zero_binary_storage_numerically() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FIGMOVE. DATA DIVISION. WORKING-STORAGE SECTION. 01 BINARY-X PIC S9(4) COMP VALUE 5. 01 DISPLAY-X PIC 9(4). 01 HIGH-X PIC X(2). 01 LOW-X PIC X(2). PROCEDURE DIVISION. MOVE ZEROES TO BINARY-X. ADD 1 TO BINARY-X. MOVE BINARY-X TO DISPLAY-X. DISPLAY DISPLAY-X. MOVE HIGH-VALUES TO HIGH-X. IF HIGH-X(2:1) = HIGH-VALUES DISPLAY 'HIGH' END-IF. MOVE LOW-VALUES TO LOW-X. IF LOW-X(2:1) = LOW-VALUES DISPLAY 'LOW' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"0001\nHIGH\nLOW\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn set_condition_true_writes_figurative_condition_values() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SETFIG. DATA DIVISION. WORKING-STORAGE SECTION. 01 FLAG-X PIC X(2) VALUE 'XX'. 88 EMPTY-X VALUE LOW-VALUES. PROCEDURE DIVISION. SET EMPTY-X TO TRUE. IF EMPTY-X DISPLAY 'COND' END-IF. IF FLAG-X(1:1) = LOW-VALUES AND FLAG-X(2:1) = LOW-VALUES DISPLAY 'LOW' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"COND\nLOW\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn numeric_condition_ranges_accept_leading_zeroes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. RANGE88. DATA DIVISION. WORKING-STORAGE SECTION. 01 MONTH-RAW PIC X(2) VALUE '04'. 01 MONTH-X REDEFINES MONTH-RAW PIC 99. 88 VALID-MONTH VALUE 1 THROUGH 12. PROCEDURE DIVISION. IF VALID-MONTH DISPLAY 'VALID' END-IF. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"VALID\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn qualification_can_name_an_outer_ancestor() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. QUALIFY. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT-X. 05 MID-X. 10 VALUE-X PIC X(2) VALUE 'OK'. 01 OUT-X PIC X(2). PROCEDURE DIVISION. MOVE VALUE-X OF ROOT-X TO OUT-X. DISPLAY OUT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"OK\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn return_code_special_register_controls_completion() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. RETCODE. DATA DIVISION. WORKING-STORAGE SECTION. 01 VALUE-X PIC 9 VALUE 5. PROCEDURE DIVISION. MOVE VALUE-X TO RETURN-CODE. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.return_code, 5),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn decimal_scale_sign_rounding_precedence_and_overflow_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DECIMAL. DATA DIVISION. WORKING-STORAGE SECTION. 01 PACKED-X PIC S9(3)V99 COMP-3 VALUE -12.34. 01 RESULT-X PIC 9V99 COMP-3. 01 DISPLAY-X PIC S9(4)V99. PROCEDURE DIVISION. ADD 2.34 TO PACKED-X. MOVE PACKED-X TO DISPLAY-X. DISPLAY DISPLAY-X. COMPUTE RESULT-X ROUNDED = 1 / 3. MOVE RESULT-X TO DISPLAY-X. DISPLAY DISPLAY-X. COMPUTE RESULT-X ROUNDED = ( 2 + 3 ) * 4 / 3. MOVE RESULT-X TO DISPLAY-X. DISPLAY DISPLAY-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"00100}\n00003C\n00066G\n")
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
    fn multiline_perform_condition_accepts_end_prefixed_condition_names() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. ENDCOND. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 9 VALUE 0. 01 TOTAL PIC 99 VALUE 0. 01 END-LOOP-FLAG PIC X VALUE 'N'. 88 END-LOOP-YES VALUE 'Y'. 01 ERROR-FLAG PIC X VALUE 'N'. 88 ERROR-YES VALUE 'Y'. PROCEDURE DIVISION.\nPERFORM VARYING I FROM 1 BY 1 UNTIL I > 3 OR\n END-LOOP-YES OR ERROR-YES\n ADD I TO TOTAL\nEND-PERFORM.\nDISPLAY TOTAL.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"06\n"),
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
            "mainframe-env.reference-machine-checkpoint@6"
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
                        identities: vec![b"ABC".to_vec()],
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
    fn cics_nested_subscript_operand_reads_selected_element() {
        use mainframe_env_host_api::{CicsOperation, HostRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSINDEX. DATA DIVISION. WORKING-STORAGE SECTION. 01 IDX PIC 9 VALUE 2. 01 NAMES. 05 PGM-NAME PIC X(4) OCCURS 2. PROCEDURE DIVISION. MOVE 'P001' TO PGM-NAME(1). MOVE 'P002' TO PGM-NAME(2). EXEC CICS INQUIRE PROGRAM(PGM-NAME(IDX)) NOHANDLE END-EXEC. STOP RUN.";
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
            panic!("CICS INQUIRE did not call host");
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("unexpected host request");
        };
        assert_eq!(request.operation, CicsOperation::Inquire);
        assert_eq!(request.arguments["PROGRAM"].bytes(), b"P002");
    }

    #[test]
    fn sql_host_variables_use_typed_db2_request() {
        use mainframe_env_host_api::{
            Db2Operation, Db2Result, Db2Row, EffectResult, HostRequest, HostResult,
        };

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
        let HostRequest::Db2(request) = &effect.request else {
            panic!("unexpected SQL request");
        };
        assert_eq!(request.operation, Db2Operation::Select);
        assert_eq!(request.inputs["IN-X"].value, b"42");
        assert_eq!(request.outputs, vec!["OUT-X"]);
        assert!(request.statement.contains("CUSTOMER"));
        assert!(matches!(
            machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Db2(Db2Result {
                        sqlcode: 0,
                        sqlstate: "00000".into(),
                        message: "ROW".into(),
                        rows: vec![Db2Row {
                            columns: vec![b"ANN".to_vec()]
                        }],
                        affected_rows: 0,
                    })),
                }),
                Quantum::new(64, 1024).unwrap(),
            ),
            MachineDrive::Completed(done) if done.output.bytes() == b"ANN\n"
        ));
    }

    #[test]
    fn sql_numeric_outputs_use_cobol_receiver_encoding() {
        use mainframe_env_host_api::{
            Db2Operation, Db2Result, Db2Row, EffectResult, HostRequest, HostResult,
        };

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SQLCOUNT. DATA DIVISION. WORKING-STORAGE SECTION. 01 COUNT-X PIC S9(4) COMP-3. 01 DISPLAY-X PIC 9(4). PROCEDURE DIVISION. EXEC SQL SELECT COUNT(1) INTO :COUNT-X FROM CUSTOMER END-EXEC. MOVE COUNT-X TO DISPLAY-X. DISPLAY DISPLAY-X. STOP RUN.";
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
            panic!("SQL COUNT did not call host");
        };
        assert!(matches!(
            &effect.request,
            HostRequest::Db2(request) if request.operation == Db2Operation::Count
        ));
        assert!(matches!(
            machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Db2(Db2Result {
                        sqlcode: 0,
                        sqlstate: "00000".into(),
                        message: "COUNT".into(),
                        rows: vec![Db2Row {
                            columns: vec![b"7".to_vec()]
                        }],
                        affected_rows: 0,
                    })),
                }),
                Quantum::new(64, 1024).unwrap(),
            ),
            MachineDrive::Completed(done) if done.output.bytes() == b"0007\n"
        ));
    }

    #[test]
    fn dli_pcb_and_ssa_are_typed_read_write_operands() {
        use mainframe_env_host_api::{HostRequest, ImsOperation};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DLIABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 PCB-N PIC S9(4) COMP VALUE 1. 01 ROOT-X PIC X(100). 01 ACCT-X PIC X(6) VALUE '000123'. PROCEDURE DIVISION. EXEC DLI GU USING PCB(PCB-N) SEGMENT(PAUTSUM0) INTO(ROOT-X) WHERE(ACCNTID = ACCT-X) END-EXEC. STOP RUN.";
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
        let HostRequest::Ims(request) = effect.request else {
            panic!("DLI did not lower to the typed IMS request");
        };
        assert_eq!(request.operation, ImsOperation::GetUnique);
        assert_eq!(request.pcb, 1);
        assert_eq!(request.segments, ["PAUTSUM0"]);
        assert_eq!(request.qualifiers[0].field, "ACCNTID");
        assert_eq!(request.qualifiers[0].value, b"000123");
    }

    #[test]
    fn mq_call_parameter_list_carries_names_modes_and_values() {
        use mainframe_env_host_api::{HostRequest, MqOperation};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MQABI. DATA DIVISION. WORKING-STORAGE SECTION. 01 HCONN PIC S9(9) COMP VALUE 0. 01 MQOD. 05 MQOD-OBJECTNAME PIC X(48) VALUE 'CARD.DEMO.REQUEST'. 01 OPTS PIC S9(9) COMP VALUE 1. 01 HOBJ PIC S9(9) COMP VALUE 0. 01 CC PIC S9(9) COMP VALUE 0. 01 RC PIC S9(9) COMP VALUE 0. PROCEDURE DIVISION. CALL 'MQOPEN' USING HCONN MQOD OPTS HOBJ CC RC. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let drive = machine.drive(MachineResume::Start, Quantum::new(64, 1024).unwrap());
        let MachineDrive::HostCall(effect) = drive else {
            panic!("MQ call did not call host: {drive:?}");
        };
        let HostRequest::Mq(request) = effect.request else {
            panic!("MQOPEN did not lower to the typed MQ request");
        };
        assert_eq!(request.operation, MqOperation::Open);
        assert_eq!(request.queue.as_deref(), Some("CARD.DEMO.REQUEST"));
        assert_eq!(request.options, 1);
    }

    #[test]
    fn cics_response_updates_into_resp_and_eib_storage() {
        use mainframe_env_host_api::{CicsDisposition, CicsResponse, EffectResult, HostResult};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSRESULT. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(3). 01 RESP-X PIC 99. 01 RESP2-X PIC 99. 01 EIBRESP PIC 99. 01 EIBRESP2 PIC 99. 01 EIBCALEN PIC 99. 01 EIBAID PIC X. 01 EIBTRNID PIC X(4). PROCEDURE DIVISION. EXEC CICS READ DATASET('D') INTO(DATA-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. DISPLAY DATA-X. DISPLAY RESP-X. DISPLAY RESP2-X. DISPLAY EIBCALEN. DISPLAY EIBTRNID. STOP RUN.";
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
            panic!("CICS read did not call host");
        };
        let payload = mainframe_env_execution_api::BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            b"ABC".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let response = CicsResponse {
            disposition: CicsDisposition::Complete,
            condition: "NOTFND".into(),
            response: 13,
            response2: 2,
            applid: "APP".into(),
            sysid: "SYS".into(),
            transaction: "T001".into(),
            aid: 0x7d,
            target: None,
            next_transaction: None,
            payload,
            outputs: BTreeMap::new(),
            unit_of_work: None,
        };
        let result = machine.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response)),
            }),
            Quantum::new(64, 1024).unwrap(),
        );
        assert!(
            matches!(
                result,
                MachineDrive::Completed(ref done)
                    if done.output.bytes() == b"ABC\n13\n02\n00\nT001\n"
            ),
            "{result:?}"
        );
    }

    #[test]
    fn cics_decimal_outputs_update_exact_cobol_destinations() {
        use mainframe_env_host_api::{CicsDisposition, CicsResponse, EffectResult, HostResult};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSTIME. DATA DIVISION. WORKING-STORAGE SECTION. 01 ABS-X PIC S9(15) COMP-3 VALUE 0. 01 ABS-DISPLAY PIC 9(15). PROCEDURE DIVISION. EXEC CICS ASKTIME ABSTIME(ABS-X) END-EXEC. MOVE ABS-X TO ABS-DISPLAY. DISPLAY ABS-DISPLAY. STOP RUN.";
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
            panic!("ASKTIME did not call host");
        };
        let response = CicsResponse {
            disposition: CicsDisposition::Complete,
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            applid: "APP".into(),
            sysid: "SYS".into(),
            transaction: "T001".into(),
            aid: 0,
            target: None,
            next_transaction: None,
            payload: mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .unwrap(),
            outputs: BTreeMap::from([(
                "ABSTIME".into(),
                mainframe_env_execution_api::BoundedPayload::new(
                    "mainframe-env.cics.decimal@1",
                    b"3997082096789".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            )]),
            unit_of_work: None,
        };
        assert!(matches!(
            machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Cics(response)),
                }),
                Quantum::new(64, 1024).unwrap(),
            ),
            MachineDrive::Completed(done) if done.output.bytes() == b"003997082096789\n"
        ));
    }

    #[test]
    fn cics_local_handler_and_entry_commarea_preserve_control_and_bytes() {
        use mainframe_env_host_api::{CicsDisposition, CicsResponse, EffectResult, HostResult};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSHANDLER. DATA DIVISION. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). PROCEDURE DIVISION USING DFHCOMMAREA. EXEC CICS INQUIRE PROGRAM('MISSING') END-EXEC. DISPLAY 'BAD'. GO TO DONE. HANDLER. DISPLAY DFHCOMMAREA. DONE. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cics.commarea".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.commarea@1",
                b"KEEP".to_vec(),
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
            panic!("INQUIRE did not call host");
        };
        let response = CicsResponse {
            disposition: CicsDisposition::Handler,
            condition: "PGMIDERR".into(),
            response: 27,
            response2: 0,
            applid: "APP".into(),
            sysid: "SYS".into(),
            transaction: "T001".into(),
            aid: 0,
            target: Some("HANDLER".into()),
            next_transaction: None,
            payload: mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .unwrap(),
            outputs: BTreeMap::new(),
            unit_of_work: None,
        };
        let result = machine.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response)),
            }),
            Quantum::new(64, 1024).unwrap(),
        );
        assert!(
            matches!(
                result,
                MachineDrive::Completed(ref done) if done.output.bytes() == b"KEEP\n"
            ),
            "{result:?}"
        );
    }

    #[test]
    fn cics_entry_eib_context_survives_storage_initialization() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. EIBENTRY. DATA DIVISION. WORKING-STORAGE SECTION. 01 EIBCALEN PIC 9(4) VALUE 0. 01 EIBAID PIC X VALUE SPACE. 01 EIBTRNID PIC X(4) VALUE SPACES. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). PROCEDURE DIVISION. DISPLAY EIBCALEN. DISPLAY EIBTRNID. DISPLAY DFHCOMMAREA. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cics.commarea".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.commarea@1",
                b"KEEP".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        invocation.bindings.insert(
            "cics.transaction".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.transaction@1",
                b"CC00".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        assert!(matches!(
            drive_to_terminal(&mut machine),
            MachineDrive::Completed(done)
                if done.output.bytes() == b"0004\nCC00\nKEEP\n"
        ));
    }
    #[test]
    fn grouped_dfhcommarea_entry_bytes_survive_child_initialization() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. COMMAREA. DATA DIVISION. WORKING-STORAGE SECTION. 01 OUT-X PIC X. LINKAGE SECTION. 01 DFHCOMMAREA. 05 PREFIX-X PIC X. 05 CONTEXT-X PIC X. PROCEDURE DIVISION. MOVE CONTEXT-X OF DFHCOMMAREA TO OUT-X. DISPLAY OUT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cics.commarea".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cics.commarea@1",
                b"A1".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(resume, Quantum::new(8, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::Completed(done) => {
                    assert_eq!(done.output.bytes(), b"1\n");
                    break;
                }
                other => panic!("{other:?}"),
            }
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
