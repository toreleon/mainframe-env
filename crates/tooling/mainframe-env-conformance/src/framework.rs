//! Shared implementation for independent selected-route fixtures.

#![forbid(unsafe_code)]

use crate::{
    abi, carddemo, cics_licensed, cics_pilot, cobol_assurance, cobol_clauses, cobol_conditions,
    cobol_data, cobol_exit, cobol_files, cobol_frontend, cobol_function_boundaries,
    cobol_functions, cobol_intrinsics, cobol_licensed, cobol_move_pilot, cobol_phrases,
    cobol_recovery, cobol_reference, cobol_registers, cobol_runtime, cobol_statements, dataset,
    dataset_reference, jcl, racf, racf_oracle,
};
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
pub use cics_licensed::{
    CicsOracleCapture, CicsOracleExpectation, CicsOracleImport, CicsOracleObservation,
    import_cics_oracle_capture,
};
pub use cics_pilot::{
    CicsPilotReport, CicsPilotRuntime, cics_pilot_runtime, run_cics_pilot_profiles,
};
pub use dataset::{
    DatasetConformanceRuntime, dataset_conformance_runtime, run_dataset_conformance,
};
pub use dataset_reference::{DatasetReferenceSimulationReport, run_dataset_reference_simulation};
pub use jcl::{JclExitReceipt, JclFixtureRuntime, jcl_fixture_runtime, verify_jcl_exit};
pub use racf::{racf_runtime, racf_runtime_with};
pub use racf_oracle::{RACF_ORACLE_RELATIVE_PATH, RacfOracleCampaign, RacfOracleCase};

pub use cobol_assurance::verify_cobol_assurance_sources;
pub use cobol_clauses::verify_cobol_semantic_fixtures;
pub use cobol_conditions::verify_cobol_condition_fixtures;
pub use cobol_data::verify_cobol_data_runtime_fixtures;
pub use cobol_exit::{CobolExitReceipt, verify_cobol_exit};
pub use cobol_files::verify_cobol_file_runtime_fixtures;
pub use cobol_frontend::{
    CobolConformanceHandlers, cobol_conformance_handlers, cobol_frontend_runtime,
    verify_cobol_frontend_fixtures,
};
pub use cobol_function_boundaries::verify_cobol_function_boundary_runtime_fixtures;
pub use cobol_functions::verify_cobol_function_fixtures;
pub use cobol_intrinsics::verify_cobol_function_runtime_fixtures;
pub use cobol_licensed::{licensed_fixture_digest, verify_cobol_licensed_receipt_from_env};
pub use cobol_move_pilot::{
    CobolMovePilotReport, CobolMovePilotRuntime, cobol_move_pilot_runtime, run_cobol_move_pilot,
};
pub use cobol_phrases::verify_cobol_statement_phrase_runtime_fixtures;
pub use cobol_recovery::verify_cobol_recovery_fixtures;
pub use cobol_reference::{
    GnuCobolReferenceReceipt, gnucobol_reference_fixture_digest, run_gnucobol_reference_campaign,
    verify_gnucobol_reference_allowlist,
};
pub use cobol_registers::verify_cobol_register_runtime_fixtures;
pub use cobol_runtime::verify_cobol_statement_runtime_fixtures;
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
        ArtifactRef::new(artifact.content_id().to_reference(), l).unwrap(),
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
    fn display_upon_and_no_advancing_do_not_leak_control_operands() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. DISPLAYOPT. PROCEDURE DIVISION. DISPLAY 'A' UPON CONSOLE WITH NO ADVANCING. DISPLAY 'B'. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"AB\n"),
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
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. POINTERS.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 TARGET-X PIC X VALUE 'A'.\n01 PTR POINTER.\nLINKAGE SECTION.\n01 BLOCK PIC X.\nPROCEDURE DIVISION.\nSET PTR TO ADDRESS OF TARGET-X.\nSET ADDRESS OF BLOCK TO PTR.\nMOVE 'Z' TO BLOCK.\nDISPLAY TARGET-X.\nSTOP RUN.\n",
        )
        .unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"Z\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn allocate_and_free_own_bounded_storage_through_a_linkage_alias() {
        let artifact = compile(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HEAP.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 PTR POINTER.\n01 OUT-X PIC X(4).\nLINKAGE SECTION.\n01 BLOCK PIC X(4).\nPROCEDURE DIVISION.\nALLOCATE 4 CHARACTERS RETURNING PTR.\nSET ADDRESS OF BLOCK TO PTR.\nMOVE 'HEAP' TO BLOCK.\nMOVE BLOCK TO OUT-X.\nFREE PTR.\nDISPLAY OUT-X.\nSTOP RUN.\n",
        )
        .unwrap();
        let invocation = invocation(&artifact, 1024);
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        match drive_to_terminal(&mut machine) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"HEAP\n"),
            other => panic!("{other:?}"),
        }
        assert_eq!(machine.variable("PTR").unwrap().bytes(), [0; 4]);
        assert!(machine.variable("BLOCK").is_none());
    }
    #[test]
    fn allocate_linkage_target_initializes_declared_values_and_can_omit_returning() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. HEAPINIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR POINTER. LINKAGE SECTION. 01 BLOCK. 05 TEXT-X PIC X(2) VALUE 'AB'. 05 NUM-X PIC 99 VALUE 12. PROCEDURE DIVISION. ALLOCATE BLOCK INITIALIZED. DISPLAY BLOCK. SET PTR TO ADDRESS OF BLOCK. FREE PTR. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"AB12\n"),
            other => panic!("{other:?}"),
        }

        let rounded = "IDENTIFICATION DIVISION. PROGRAM-ID. HEAPROUND. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR POINTER. LINKAGE SECTION. 01 BLOCK PIC X(2). PROCEDURE DIVISION. ALLOCATE 1.1 CHARACTERS INITIALIZED RETURNING PTR. SET ADDRESS OF BLOCK TO PTR. MOVE 'OK' TO BLOCK. DISPLAY BLOCK. FREE PTR. STOP RUN.";
        let artifact = compile(rounded).unwrap();
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
    fn search_all_selects_the_matching_sorted_table_element() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SEARCHALL. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT-X. 05 TABLE-X OCCURS 4 TIMES ASCENDING KEY IS VALUE-X INDEXED BY IDX. 10 VALUE-X PIC X. PROCEDURE DIVISION. MOVE 'A' TO VALUE-X(1). MOVE 'B' TO VALUE-X(2). MOVE 'C' TO VALUE-X(3). MOVE 'D' TO VALUE-X(4). SEARCH ALL TABLE-X AT END DISPLAY 'MISS' WHEN VALUE-X(IDX) = 'C' DISPLAY IDX END-SEARCH. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"3\n"),
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
    fn bounded_dynamic_storage_publishes_and_executes() {
        let artifact = compile("IDENTIFICATION DIVISION. PROGRAM-ID. X. DATA DIVISION. WORKING-STORAGE SECTION. 01 DYNAMIC-X PIC X DYNAMIC LENGTH LIMIT IS 64. PROCEDURE DIVISION. MOVE 'OK' TO DYNAMIC-X. DISPLAY DYNAMIC-X. STOP RUN.").unwrap();
        assert!(
            matches!(execute(&artifact, 1024), MachineDrive::Completed(done) if done.output.bytes() == b"OK\n")
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
    fn arithmetic_conformance_programs_use_the_typed_decimal_dialect() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. TYPEDMATH. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 2. 01 B PIC 99 VALUE 3. 01 C PIC 99 VALUE 0. 01 D PIC 99 VALUE 0. 01 GOOD-X PIC 9 VALUE 4. 01 SMALL-X PIC 9 VALUE 9. PROCEDURE DIVISION. ADD A TO B. COMPUTE C = A + B. COMPUTE D ROUNDED = B / 2. ADD A TO GOOD-X SMALL-X ON SIZE ERROR CONTINUE END-ADD. DISPLAY B C D GOOD-X SMALL-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        assert!(
            artifact
                .manifest()
                .dialect_contracts
                .contains("mainframe.decimal@1")
        );
        let module = mainframe_env_ir::decode_binary(
            artifact.payload(),
            mainframe_env_ir::CodecLimits::default(),
        )
        .unwrap();
        let operations = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .collect::<Vec<_>>();
        assert!(!operations.iter().any(|operation| {
            operation.identity.namespace() == "mainframe.core.cobol"
                && matches!(operation.identity.name(), "add" | "compute")
        }));
        let typed = operations
            .iter()
            .filter(|operation| {
                operation.identity.namespace() == "mainframe.decimal"
                    && operation.identity.name() == "assign"
                    && operation.identity.major() == 1
            })
            .collect::<Vec<_>>();
        assert_eq!(typed.len(), 4);
        assert!(typed.iter().all(|operation| {
            operation.attributes.contains_key("assignment_plan")
                && !operation.attributes.contains_key("arguments")
                && !operation.attributes.contains_key("control_text")
        }));
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"05070349\n"
        ));
    }
    #[test]
    fn every_publishable_layout_category_decodes_and_constructs_the_machine() {
        use mainframe_env_ir::{Attribute, decode_binary};

        assert_eq!(
            mainframe_env_compiler::PUBLISHABLE_LAYOUT_CATEGORIES,
            mainframe_env_interpreter::SUPPORTED_LAYOUT_CATEGORIES
        );
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CATEGORIES. DATA DIVISION. WORKING-STORAGE SECTION. 01 ROOT. 05 ALPHA PIC A(3). 05 ALNUM PIC X(3). 05 ALNUM-EDIT PIC XX/XX. 05 DBCS-ITEM PIC G(2) DISPLAY-1. 05 NATIONAL-ITEM PIC N(2) NATIONAL. 05 NATIONAL-EDIT PIC 99/99 NATIONAL. 05 UTF8-ITEM PIC U(3) BYTE-LENGTH 9 UTF-8. 05 NUM-DISPLAY PIC 9(3). 05 NUM-EDIT PIC ZZ9. 05 PACKED PIC 9(3) COMP-3. 05 BINARY-X PIC 9(4) BINARY. 05 SHORT-FLOAT COMP-1. 05 LONG-FLOAT COMP-2. 05 INDEX-ITEM INDEX. 05 DATA-PTR POINTER. 05 PTR32 POINTER-32. 05 PROC-PTR PROCEDURE-POINTER. 05 FUNC-PTR FUNCTION-POINTER. 05 OBJ OBJECT REFERENCE. 05 FLAG PIC 9. 88 FLAG-ON VALUE 1. 66 ROOT-ALIAS RENAMES ALPHA THRU ALNUM. 01 NATIONAL-ROOT GROUP-USAGE NATIONAL. 05 NATIONAL-CHAR PIC N(2). 01 UTF8-ROOT GROUP-USAGE UTF-8. 05 UTF8-CHAR PIC U(2) BYTE-LENGTH 6. PROCEDURE DIVISION. STOP RUN.";
        let artifact = compile(source).unwrap();
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let categories = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .filter(|operation| operation.identity.name() == "define")
            .filter_map(|operation| match operation.attributes.get("category") {
                Some(Attribute::Text(category)) => Some(category.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            categories,
            mainframe_env_compiler::PUBLISHABLE_LAYOUT_CATEGORIES
                .iter()
                .copied()
                .collect()
        );
        ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .expect("every compiler layout category must construct the reference machine");
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
    fn relational_start_and_delete_emit_exact_typed_dataset_effects() {
        use mainframe_env_host_api::{
            DatasetRequest, DatasetResult, EffectResult, HostRequest, HostResult, KeyRelation,
        };

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. FILEOPS. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED ACCESS MODE IS DYNAMIC RECORD KEY IS REC-KEY FILE STATUS IS FILE-STATUS. DATA DIVISION. FILE SECTION. FD TEST-FILE. 01 TEST-RECORD. 05 REC-KEY PIC X(2) VALUE 'BB'. 05 REC-VALUE PIC X(2) VALUE '02'. WORKING-STORAGE SECTION. 01 FILE-STATUS PIC XX. PROCEDURE DIVISION. START TEST-FILE KEY IS NOT LESS THAN REC-KEY INVALID KEY DISPLAY 'BAD-START' END-START. DELETE TEST-FILE RECORD INVALID KEY DISPLAY 'BAD-DELETE' END-DELETE. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cobol.dd.TEST-FILE".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.dataset-name@1",
                b"USER.TEST".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let mut resume = MachineResume::Start;
        let mut effects = 0usize;
        loop {
            match machine.drive(resume, Quantum::new(128, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    effects += 1;
                    let outcome = match effect.request {
                        HostRequest::Dataset(DatasetRequest::StartBrowse {
                            dataset,
                            key,
                            relation,
                        }) => {
                            assert_eq!(dataset.as_str(), "USER.TEST");
                            assert_eq!(key, b"BB");
                            assert_eq!(relation, KeyRelation::GreaterOrEqual);
                            HostResult::Dataset(DatasetResult::Browse {
                                cursor: "cursor-1".into(),
                                record: None,
                                identity: None,
                                key: None,
                            })
                        }
                        HostRequest::Dataset(DatasetRequest::DeleteRecord {
                            dataset, key, ..
                        }) => {
                            assert_eq!(dataset.as_str(), "USER.TEST");
                            assert_eq!(key, b"BB");
                            HostResult::Dataset(DatasetResult::Mutated { version: 2 })
                        }
                        other => panic!("unexpected file effect: {other:?}"),
                    };
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: Ok(outcome),
                    });
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(effects, 2);
                    assert!(done.output.bytes().is_empty());
                    break;
                }
                other => panic!("{other:?}; position={}", machine.position_summary()),
            }
        }
    }

    #[test]
    fn sort_using_giving_orders_bounded_records_through_typed_effects() {
        use mainframe_env_host_api::{
            DatasetRequest, DatasetResult, EffectResult, HostRequest, HostResult,
        };

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SORTIO. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT INPUT-FILE ASSIGN TO INPUTDD ORGANIZATION IS SEQUENTIAL. SELECT OUTPUT-FILE ASSIGN TO OUTPUTDD ORGANIZATION IS SEQUENTIAL. DATA DIVISION. FILE SECTION. FD INPUT-FILE. 01 INPUT-RECORD PIC X(4). FD OUTPUT-FILE. 01 OUTPUT-RECORD PIC X(4). SD SORT-FILE. 01 SORT-RECORD. 05 SORT-KEY PIC X(2). 05 SORT-DATA PIC X(2). PROCEDURE DIVISION. SORT SORT-FILE ON ASCENDING KEY SORT-KEY USING INPUT-FILE GIVING OUTPUT-FILE. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        for (logical, dataset) in [
            ("INPUT-FILE", b"USER.INPUT".as_slice()),
            ("OUTPUT-FILE", b"USER.OUTPUT".as_slice()),
        ] {
            invocation.bindings.insert(
                format!("cobol.dd.{logical}"),
                mainframe_env_execution_api::BoundedPayload::new(
                    "mainframe-env.dataset-name@1",
                    dataset.to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
        }
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let mut resume = MachineResume::Start;
        let mut effects = 0usize;
        loop {
            match machine.drive(resume, Quantum::new(128, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    effects += 1;
                    let outcome = match effect.request {
                        HostRequest::Dataset(DatasetRequest::Read { dataset, .. }) => {
                            assert_eq!(dataset.as_str(), "USER.INPUT");
                            HostResult::Dataset(DatasetResult::Records {
                                records: vec![b"BB02".to_vec(), b"AA03".to_vec(), b"AA01".to_vec()],
                                identities: vec![b"BB".to_vec(), b"AA".to_vec(), b"AA".to_vec()],
                                version: 1,
                            })
                        }
                        HostRequest::Dataset(DatasetRequest::Write {
                            dataset, records, ..
                        }) => {
                            assert_eq!(dataset.as_str(), "USER.OUTPUT");
                            assert_eq!(
                                records,
                                vec![b"AA03".to_vec(), b"AA01".to_vec(), b"BB02".to_vec()]
                            );
                            HostResult::Dataset(DatasetResult::Mutated { version: 2 })
                        }
                        other => panic!("unexpected sort effect: {other:?}"),
                    };
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: Ok(outcome),
                    });
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(effects, 2);
                    assert!(done.output.bytes().is_empty());
                    break;
                }
                other => panic!("{other:?}; position={}", machine.position_summary()),
            }
        }
    }

    #[test]
    fn sort_input_output_procedures_release_and_return_in_key_order() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SORTPROC. DATA DIVISION. FILE SECTION. SD SORT-FILE. 01 SORT-RECORD. 05 SORT-KEY PIC X(2). 05 SORT-DATA PIC X(2). PROCEDURE DIVISION. SORT SORT-FILE ON ASCENDING KEY SORT-KEY INPUT PROCEDURE FEED OUTPUT PROCEDURE DRAIN. STOP RUN. FEED. MOVE 'BB02' TO SORT-RECORD. RELEASE SORT-RECORD. MOVE 'AA01' TO SORT-RECORD. RELEASE SORT-RECORD. EXIT. DRAIN. RETURN SORT-FILE RECORD INTO SORT-RECORD AT END DISPLAY 'EMPTY' END-RETURN. DISPLAY SORT-RECORD. EXIT.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"AA01\n"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn release_from_and_return_into_preserve_sort_record_routes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SORTFROM. DATA DIVISION. FILE SECTION. SD SORT-FILE. 01 SORT-RECORD PIC X(4). WORKING-STORAGE SECTION. 01 SOURCE-X PIC X(4) VALUE 'AA01'. 01 OUT-X PIC X(4). PROCEDURE DIVISION. SORT SORT-FILE ON ASCENDING KEY SORT-RECORD INPUT PROCEDURE FEED OUTPUT PROCEDURE DRAIN. STOP RUN. FEED. RELEASE SORT-RECORD FROM SOURCE-X. EXIT. DRAIN. RETURN SORT-FILE RECORD INTO OUT-X AT END DISPLAY 'BAD' END-RETURN. DISPLAY OUT-X. EXIT.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"AA01\n"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn invoke_uses_typed_class_method_abi_and_applies_returning_value() {
        use mainframe_env_host_api::{EffectResult, HostRequest, HostResult, ProgramRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. OBJECTCALL. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECEIVER OBJECT REFERENCE CUSTOMER. 01 ARG-X PIC X(3) VALUE 'IN'. 01 RESULT-X PIC X(3). PROCEDURE DIVISION. SET RECEIVER TO ADDRESS OF ARG-X. INVOKE RECEIVER 'RUN' USING ARG-X RETURNING RESULT-X ON EXCEPTION DISPLAY 'BAD' END-INVOKE. DISPLAY ARG-X. DISPLAY RESULT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(resume, Quantum::new(128, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    match &effect.request {
                        HostRequest::Program(ProgramRequest::Invoke {
                            class,
                            method,
                            receiver,
                            payload,
                        }) => {
                            assert_eq!(class.as_str(), "CUSTOMER");
                            assert_eq!(method.as_str(), "RUN");
                            assert_eq!(receiver.schema(), "mainframe-env.cobol-object-reference@1");
                            assert!(receiver.bytes().iter().any(|byte| *byte != 0));
                            assert_eq!(payload.schema(), "mainframe-env.cobol.call@1");
                        }
                        other => panic!("unexpected INVOKE effect: {other:?}"),
                    }
                    let result = mainframe_env_interpreter::encode_cobol_call_result(&[
                        b"OUT".to_vec(),
                        b"RET".to_vec(),
                    ])
                    .unwrap();
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: Ok(HostResult::Program(result)),
                    });
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(done.output.bytes(), b"OUT\nRET\n");
                    break;
                }
                other => panic!("{other:?}; position={}", machine.position_summary()),
            }
        }
    }

    #[test]
    fn computed_go_to_exit_program_and_stop_returning_preserve_control_results() {
        let selected = compile("IDENTIFICATION DIVISION. PROGRAM-ID. GOTODEP. DATA DIVISION. WORKING-STORAGE SECTION. 01 SELECTOR-X PIC 9 VALUE 2. PROCEDURE DIVISION. GO TO FIRST-P SECOND-P DEPENDING ON SELECTOR-X. DISPLAY 'FALL'. STOP RUN. FIRST-P. DISPLAY 'ONE'. STOP RUN. SECOND-P. DISPLAY 'TWO'. EXIT PROGRAM. DISPLAY 'BAD'.")
            .unwrap();
        assert!(matches!(
            execute(&selected, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"TWO\n"
        ));

        let fallthrough = compile("IDENTIFICATION DIVISION. PROGRAM-ID. GOTOFALL. DATA DIVISION. WORKING-STORAGE SECTION. 01 SELECTOR-X PIC 9 VALUE 3. PROCEDURE DIVISION. GO TO FIRST-P SECOND-P DEPENDING ON SELECTOR-X. DISPLAY 'FALL'. STOP RUN. FIRST-P. DISPLAY 'BAD1'. STOP RUN. SECOND-P. DISPLAY 'BAD2'. STOP RUN.")
            .unwrap();
        assert!(matches!(
            execute(&fallthrough, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"FALL\n"
        ));

        let returning = compile("IDENTIFICATION DIVISION. PROGRAM-ID. STOPCODE. PROCEDURE DIVISION. STOP RUN RETURNING 7.").unwrap();
        assert!(matches!(
            execute(&returning, 1024),
            MachineDrive::Completed(done) if done.return_code == 7
        ));
    }

    #[test]
    fn accept_clock_and_environment_inputs_are_explicit_typed_effects() {
        use mainframe_env_host_api::{ClockRequest, EffectResult, HostRequest, HostResult};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. ACCEPTS. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATE-X PIC X(8). 01 DAY-X PIC X(7). 01 WEEKDAY-X PIC X. 01 TIME-X PIC X(8). 01 ENV-X PIC X(4). PROCEDURE DIVISION. ACCEPT DATE-X FROM DATE YYYYMMDD. ACCEPT DAY-X FROM DAY YYYYDDD. ACCEPT WEEKDAY-X FROM DAY-OF-WEEK. ACCEPT TIME-X FROM TIME. ACCEPT ENV-X FROM ENVIRONMENT 'MODE'. DISPLAY DATE-X. DISPLAY DAY-X. DISPLAY WEEKDAY-X. DISPLAY TIME-X. DISPLAY ENV-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cobol.environment.MODE".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cobol.environment@1",
                b"TEST".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(resume, Quantum::new(128, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    let value = match effect.request {
                        HostRequest::Clock(ClockRequest::Date) => "20240229",
                        HostRequest::Clock(ClockRequest::Time) => "123456789",
                        other => panic!("unexpected ACCEPT effect: {other:?}"),
                    };
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: Ok(HostResult::Clock(value.into())),
                    });
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(
                        done.output.bytes(),
                        b"20240229\n2024060\n4\n12345678\nTEST\n"
                    );
                    break;
                }
                other => panic!("{other:?}; position={}", machine.position_summary()),
            }
        }
    }

    #[test]
    fn cancel_resolves_every_literal_and_dynamic_program_in_source_order() {
        use mainframe_env_host_api::{EffectResult, HostRequest, HostResult, ProgramRequest};

        let artifact = compile("IDENTIFICATION DIVISION. PROGRAM-ID. CANCELS. DATA DIVISION. WORKING-STORAGE SECTION. 01 PROGRAM-X PIC X(3) VALUE 'TWO'. PROCEDURE DIVISION. CANCEL 'ONE' PROGRAM-X. STOP RUN.").unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(resume, Quantum::new(128, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    match effect.request {
                        HostRequest::Program(ProgramRequest::Cancel { programs }) => assert_eq!(
                            programs
                                .iter()
                                .map(|program| program.as_str())
                                .collect::<Vec<_>>(),
                            ["ONE", "TWO"]
                        ),
                        other => panic!("unexpected CANCEL effect: {other:?}"),
                    }
                    let payload = mainframe_env_execution_api::BoundedPayload::new(
                        "mainframe-env.program.cancel@1",
                        Vec::new(),
                        InvocationLimits::default(),
                    )
                    .unwrap();
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: Ok(HostResult::Program(payload)),
                    });
                }
                MachineDrive::Completed(_) => break,
                other => panic!("{other:?}"),
            }
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
    fn add_direct_giving_to_giving_and_size_error_preserve_exact_destinations() {
        let direct = compile(
            "IDENTIFICATION DIVISION. PROGRAM-ID. ADDGIVE. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 2. 01 B PIC 9 VALUE 4. 01 C PIC 9 VALUE 0. PROCEDURE DIVISION. ADD A GIVING C. DISPLAY C. ADD A TO B GIVING C. DISPLAY B C. STOP RUN.",
        )
        .unwrap();
        assert!(matches!(
            execute(&direct, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"2\n46\n"
        ));

        let size_error = compile(
            "IDENTIFICATION DIVISION. PROGRAM-ID. ADDSIZE. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 5. 01 B PIC 9 VALUE 8. 01 C PIC 9 VALUE 4. PROCEDURE DIVISION. ADD B C GIVING A ON SIZE ERROR DISPLAY 'SIZE' NOT ON SIZE ERROR DISPLAY 'BAD' END-ADD. DISPLAY A. STOP RUN.",
        )
        .unwrap();
        match execute(&size_error, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"SIZE\n5\n"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multiple_arithmetic_receivers_are_atomic_on_size_error() {
        let normal = compile("IDENTIFICATION DIVISION. PROGRAM-ID. MULTIRECV. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. 01 B PIC 9 VALUE 2. 01 C PIC 9 VALUE 3. PROCEDURE DIVISION. ADD A TO B C. SUBTRACT A FROM B C. DISPLAY B C. STOP RUN.").unwrap();
        assert!(matches!(
            execute(&normal, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"23\n"
        ));

        let size_error = compile("IDENTIFICATION DIVISION. PROGRAM-ID. MULTISIZE. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 9 VALUE 1. 01 B PIC 9 VALUE 9. 01 C PIC 9 VALUE 5. PROCEDURE DIVISION. ADD A TO B C ON SIZE ERROR DISPLAY 'SIZE' END-ADD. DISPLAY B C. STOP RUN.").unwrap();
        assert!(matches!(
            execute(&size_error, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"SIZE\n95\n"
        ));
    }

    #[test]
    fn add_corresponding_matches_numeric_descendants_atomically() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CORR. DATA DIVISION. WORKING-STORAGE SECTION. 01 SOURCE-G. 05 COUNT-X PIC 99 VALUE 2. 05 NESTED-G. 10 AMOUNT-X PIC 99 VALUE 3. 05 TEXT-X PIC X VALUE 'S'. 01 TARGET-G. 05 COUNT-X PIC 99 VALUE 10. 05 NESTED-G. 10 AMOUNT-X PIC 99 VALUE 20. 05 TEXT-X PIC X VALUE 'T'. PROCEDURE DIVISION. ADD CORRESPONDING SOURCE-G TO TARGET-G. DISPLAY COUNT-X OF TARGET-G AMOUNT-X OF NESTED-G OF TARGET-G TEXT-X OF TARGET-G. STOP RUN.";
        let artifact = compile(source).unwrap();
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"1223T\n"
        ));
    }

    #[test]
    fn move_corresponding_converts_matching_elementary_descendants() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MOVECORR. DATA DIVISION. WORKING-STORAGE SECTION. 01 SOURCE-G. 05 COUNT-X PIC 9(3) COMP-3 VALUE 12. 05 NESTED-G. 10 TEXT-X PIC X(2) VALUE 'OK'. 05 ONLY-SOURCE PIC X VALUE 'S'. 01 TARGET-G. 05 COUNT-X PIC 9(4) VALUE 0. 05 NESTED-G. 10 TEXT-X PIC X(3) VALUE 'BAD'. 05 ONLY-TARGET PIC X VALUE 'T'. PROCEDURE DIVISION. MOVE CORRESPONDING SOURCE-G TO TARGET-G. DISPLAY COUNT-X OF TARGET-G TEXT-X OF NESTED-G OF TARGET-G ONLY-TARGET. STOP RUN.";
        let artifact = compile(source).unwrap();
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"0012OK T\n"
        ));
    }

    #[test]
    fn multiply_giving_and_divide_remainder_preserve_source_receivers() {
        let artifact = compile("IDENTIFICATION DIVISION. PROGRAM-ID. MULDIV. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 2. 01 B PIC 99 VALUE 7. 01 C PIC 99 VALUE 0. 01 R PIC 99 VALUE 0. PROCEDURE DIVISION. MULTIPLY A BY B GIVING C. DISPLAY B C. DIVIDE B BY A GIVING C REMAINDER R. DISPLAY B C R. STOP RUN.").unwrap();
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Completed(done) if done.output.bytes() == b"0714\n070301\n"
        ));
    }
    #[test]
    fn condition_families_ignore_conflicting_prior_file_status() {
        use mainframe_env_host_api::{DatasetResult, EffectResult, HostRequest, HostResult};

        fn run(source: &str, records: Vec<Vec<u8>>) -> Vec<u8> {
            let artifact = compile(source).unwrap();
            let mut invocation = invocation(&artifact, 4096);
            invocation.bindings.insert(
                "cobol.dd.INPUT-FILE".into(),
                mainframe_env_execution_api::BoundedPayload::new(
                    "mainframe-env.dataset-name@1",
                    b"USER.INPUT".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            let mut machine = ReferenceMachine::from_binary(
                artifact.payload(),
                invocation,
                CodecLimits::default(),
            )
            .unwrap();
            let mut resume = MachineResume::Start;
            for _ in 0..64 {
                match machine.drive(resume, Quantum::new(256, 4096).unwrap()) {
                    MachineDrive::Continue => resume = MachineResume::Start,
                    MachineDrive::HostCall(effect) => {
                        assert!(matches!(effect.request, HostRequest::Dataset(_)));
                        resume = MachineResume::HostResult(EffectResult {
                            sequence: effect.sequence,
                            outcome: Ok(HostResult::Dataset(DatasetResult::Records {
                                identities: records.clone(),
                                records: records.clone(),
                                version: 1,
                            })),
                        });
                    }
                    MachineDrive::Completed(done) => return done.output.bytes().to_vec(),
                    other => panic!("{other:?}"),
                }
            }
            panic!("condition-family program did not terminate")
        }

        let successful = "IDENTIFICATION DIVISION. PROGRAM-ID. STATUSOK. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC PIC X. 01 A PIC 9 VALUE 1. 01 TEXT-X PIC X VALUE 'A'. 01 TARGET-X PIC X(8). 01 JSON-X PIC X(32). 01 XML-X PIC X(32). PROCEDURE DIVISION. READ INPUT-FILE INTO REC AT END CONTINUE END-READ. ADD 1 TO A ON SIZE ERROR DISPLAY 'BAD-ADD' NOT ON SIZE ERROR DISPLAY 'ADD-OK' END-ADD. STRING TEXT-X DELIMITED BY SIZE INTO TARGET-X ON OVERFLOW DISPLAY 'BAD-STR' NOT ON OVERFLOW DISPLAY 'STR-OK' END-STRING. UNSTRING TEXT-X DELIMITED BY SPACE INTO TARGET-X ON OVERFLOW DISPLAY 'BAD-UNSTR' NOT ON OVERFLOW DISPLAY 'UNSTR-OK' END-UNSTRING. JSON GENERATE JSON-X FROM TEXT-X ON EXCEPTION DISPLAY 'BAD-JSON' NOT ON EXCEPTION DISPLAY 'JSON-OK' END-JSON. XML GENERATE XML-X FROM TEXT-X ON EXCEPTION DISPLAY 'BAD-XML' NOT ON EXCEPTION DISPLAY 'XML-OK' END-XML. STOP RUN.";
        assert_eq!(
            run(successful, Vec::new()),
            b"ADD-OK\nSTR-OK\nUNSTR-OK\nJSON-OK\nXML-OK\n"
        );

        let failing = "IDENTIFICATION DIVISION. PROGRAM-ID. STATUSERR. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC PIC X. 01 A PIC 9 VALUE 9. 01 B PIC 9 VALUE 9. 01 SMALL PIC 9 VALUE 5. 01 TEXT-X PIC X(3) VALUE 'ABC'. 01 WORDS-X PIC X(3) VALUE 'A B'. 01 BAD-X PIC X(3) VALUE 'BAD'. PROCEDURE DIVISION. READ INPUT-FILE INTO REC AT END CONTINUE END-READ. ADD A B GIVING SMALL ON SIZE ERROR DISPLAY 'ADD-ERR' NOT ON SIZE ERROR DISPLAY 'BAD-ADD' END-ADD. STRING TEXT-X DELIMITED BY SIZE INTO REC ON OVERFLOW DISPLAY 'STR-ERR' NOT ON OVERFLOW DISPLAY 'BAD-STR' END-STRING. UNSTRING WORDS-X DELIMITED BY SPACE INTO REC ON OVERFLOW DISPLAY 'UNSTR-ERR' NOT ON OVERFLOW DISPLAY 'BAD-UNSTR' END-UNSTRING. JSON PARSE BAD-X INTO REC ON EXCEPTION DISPLAY 'JSON-ERR' NOT ON EXCEPTION DISPLAY 'BAD-JSON' END-JSON. XML PARSE BAD-X INTO REC ON EXCEPTION DISPLAY 'XML-ERR' NOT ON EXCEPTION DISPLAY 'BAD-XML' END-XML. STOP RUN.";
        assert_eq!(
            run(failing, vec![b"R".to_vec()]),
            b"ADD-ERR\nSTR-ERR\nUNSTR-ERR\nJSON-ERR\nXML-ERR\n"
        );
    }
    #[test]
    fn use_after_standard_error_runs_its_declarative_section_and_returns() {
        use mainframe_env_host_api::{EffectResult, HostProblem};

        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. DECLERR.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED ACCESS MODE IS RANDOM RECORD KEY IS REC-KEY.\nDATA DIVISION.\nFILE SECTION.\nFD TEST-FILE.\n01 TEST-REC.\n05 REC-KEY PIC X VALUE 'A'.\n05 DATA-X PIC X.\nPROCEDURE DIVISION.\nDECLARATIVES.\nERROR-HANDLER SECTION.\nUSE AFTER STANDARD ERROR PROCEDURE ON TEST-FILE.\nHANDLE-P.\nDISPLAY 'DECL'.\nEXIT.\nEND DECLARATIVES.\nMAIN-P.\nREAD TEST-FILE RECORD KEY IS REC-KEY.\nDISPLAY 'DONE'.\nSTOP RUN.\n";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(128, 4096).unwrap())
        else {
            panic!("declarative READ did not emit a host effect");
        };
        let mut resume = MachineResume::HostResult(EffectResult {
            sequence: effect.sequence,
            outcome: Err(HostProblem::Condition {
                name: "NOTFND".into(),
                response: 13,
                response2: 0,
            }),
        });
        loop {
            match machine.drive(resume, Quantum::new(128, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::Completed(done) => {
                    assert_eq!(done.output.bytes(), b"DECL\nDONE\n");
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn json_and_xml_generate_count_the_exact_emitted_bytes() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. GENCOUNT. DATA DIVISION. WORKING-STORAGE SECTION. 01 SOURCE-X PIC X(3) VALUE 'A&B'. 01 JSON-X PIC X(64). 01 XML-X PIC X(64). 01 JSON-N PIC 99. 01 XML-N PIC 99. PROCEDURE DIVISION. JSON GENERATE JSON-X FROM SOURCE-X COUNT IN JSON-N. XML GENERATE XML-X FROM SOURCE-X COUNT IN XML-N. DISPLAY JSON-N. DISPLAY XML-N. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"18\n28\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn xml_parse_processing_procedure_receives_ordered_events_and_through_range() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. XMLPROCESS. DATA DIVISION. WORKING-STORAGE SECTION. 01 XML-X PIC X(32) VALUE '<ROOT>A&amp;B</ROOT>'. PROCEDURE DIVISION. XML PARSE XML-X PROCESSING PROCEDURE HANDLE THRU HANDLE-END. DISPLAY 'DONE'. STOP RUN. HANDLE. DISPLAY XML-EVENT ':' XML-TEXT. HANDLE-END. EXIT. NEXT-P. DISPLAY 'BAD'.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 4096) {
            MachineDrive::Completed(done) => assert_eq!(
                done.output.bytes(),
                b"START-OF-DOCUMENT:\nSTART-OF-ELEMENT:ROOT\nCONTENT-CHARACTERS:A&B\nEND-OF-ELEMENT:ROOT\nEND-OF-DOCUMENT:\nDONE\n"
            ),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn json_parse_with_detail_emits_a_deterministic_exception_message() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. JSONDETAIL. DATA DIVISION. WORKING-STORAGE SECTION. 01 BAD-X PIC X(3) VALUE 'BAD'. 01 OUT-X PIC X(8). PROCEDURE DIVISION. JSON PARSE BAD-X INTO OUT-X WITH DETAIL ON EXCEPTION DISPLAY 'ERR' NOT ON EXCEPTION DISPLAY 'BAD' END-JSON. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(
                done.output.bytes(),
                b"IGZ0335W JSON PARSE input is invalid\nERR\n"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn call_exception_state_ignores_conflicting_prior_file_status() {
        use mainframe_env_host_api::{
            DatasetResult, EffectResult, HostProblem, HostRequest, HostResult,
        };

        fn run(records: Vec<Vec<u8>>, call: Result<HostResult, HostProblem>) -> Vec<u8> {
            let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CALLSTATUS. DATA DIVISION. WORKING-STORAGE SECTION. 01 REC PIC X. PROCEDURE DIVISION. READ INPUT-FILE INTO REC AT END CONTINUE END-READ. CALL 'SUB' ON EXCEPTION DISPLAY 'CALL-ERR' NOT ON EXCEPTION DISPLAY 'CALL-OK' END-CALL. STOP RUN.";
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
            let mut machine = ReferenceMachine::from_binary(
                artifact.payload(),
                invocation,
                CodecLimits::default(),
            )
            .unwrap();
            let mut resume = MachineResume::Start;
            let mut call = Some(call);
            for _ in 0..64 {
                match machine.drive(resume, Quantum::new(128, 1024).unwrap()) {
                    MachineDrive::Continue => resume = MachineResume::Start,
                    MachineDrive::HostCall(effect) => {
                        let outcome = match effect.request {
                            HostRequest::Dataset(_) => {
                                Ok(HostResult::Dataset(DatasetResult::Records {
                                    identities: records.clone(),
                                    records: records.clone(),
                                    version: 1,
                                }))
                            }
                            HostRequest::Program(_) => call.take().unwrap(),
                            other => panic!("unexpected host request: {other:?}"),
                        };
                        resume = MachineResume::HostResult(EffectResult {
                            sequence: effect.sequence,
                            outcome,
                        });
                    }
                    MachineDrive::Completed(done) => return done.output.bytes().to_vec(),
                    other => panic!("{other:?}"),
                }
            }
            panic!("CALL condition program did not terminate")
        }

        let empty_result = mainframe_env_execution_api::BoundedPayload::new(
            "mainframe-env.cobol.call-result@1",
            0u32.to_be_bytes().to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            run(Vec::new(), Ok(HostResult::Program(empty_result))),
            b"CALL-OK\n"
        );
        assert_eq!(
            run(vec![b"R".to_vec()], Err(HostProblem::ProviderFailure)),
            b"CALL-ERR\n"
        );
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
    fn inspect_leading_first_characters_and_delimited_ranges_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INSPECTPHRASE. DATA DIVISION. WORKING-STORAGE SECTION. 01 TEXT-X PIC X(9) VALUE '000CABACA'. 01 COUNT-X PIC 99 VALUE 1. PROCEDURE DIVISION. INSPECT TEXT-X TALLYING COUNT-X FOR LEADING '0'. INSPECT TEXT-X REPLACING FIRST 'A' BY '2' AFTER INITIAL 'C'. INSPECT TEXT-X REPLACING LEADING '0' BY 'X' BEFORE INITIAL 'C'. INSPECT TEXT-X REPLACING CHARACTERS BY '*' AFTER INITIAL 'B' BEFORE INITIAL 'C'. DISPLAY COUNT-X. DISPLAY TEXT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"04\nXXXC2B*CA\n"),
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
    fn initialize_with_filler_is_distinct_from_default_group_initialization() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INITFILL. DATA DIVISION. WORKING-STORAGE SECTION. 01 GROUP-A. 05 FILLER PIC X VALUE 'A'. 05 VALUE-A PIC X VALUE 'B'. 01 GROUP-B. 05 FILLER PIC X VALUE 'C'. 05 VALUE-B PIC X VALUE 'D'. PROCEDURE DIVISION. INITIALIZE GROUP-A. INITIALIZE GROUP-B WITH FILLER. DISPLAY GROUP-A. DISPLAY GROUP-B. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"A \n  \n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn initialize_replacing_multiple_categories_and_then_default_are_distinct() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. INITREPL. DATA DIVISION. WORKING-STORAGE SECTION. 01 GROUP-X. 05 TEXT-X PIC X(2) VALUE 'AB'. 05 NUMBER-X PIC 99 VALUE 12. 05 LETTER-X PIC A VALUE 'Z'. PROCEDURE DIVISION. INITIALIZE GROUP-X REPLACING ALPHANUMERIC DATA BY 'X' NUMERIC DATA BY 7. DISPLAY GROUP-X. MOVE 'AB' TO TEXT-X. MOVE 12 TO NUMBER-X. MOVE 'Z' TO LETTER-X. INITIALIZE GROUP-X REPLACING ALPHANUMERIC DATA BY 'X' NUMERIC DATA BY 7 THEN TO DEFAULT. DISPLAY GROUP-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"X 07Z\nX 07 \n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn subscript_reference_modification_and_bounds_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. REFS. DATA DIVISION. WORKING-STORAGE SECTION. 01 TABLE-GROUP. 05 TABLE-X PIC X(3) OCCURS 3 TIMES VALUE 'ABC'. 01 TEXT-X PIC X(6) VALUE '123456'. 01 OUT-X PIC X(3). PROCEDURE DIVISION. MOVE TABLE-X(2) TO OUT-X. DISPLAY OUT-X. MOVE 'XYZ' TO TABLE-X(3). MOVE TABLE-X(3) TO OUT-X. DISPLAY OUT-X. MOVE TEXT-X(2:3) TO OUT-X. DISPLAY OUT-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"ABC\nXYZ\n234\n")
            }
            other => panic!("{other:?}"),
        }

        let bad = "IDENTIFICATION DIVISION. PROGRAM-ID. BADREF. DATA DIVISION. WORKING-STORAGE SECTION. 01 TABLE-GROUP. 05 TABLE-X PIC X OCCURS 2 TIMES. 01 OUT-X PIC X. PROCEDURE DIVISION. MOVE TABLE-X(3) TO OUT-X. STOP RUN.";
        let artifact = compile(bad).unwrap();
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Condition(condition) if condition.name == "SUBSCRIPT-ERROR"
        ));
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
    fn out_of_line_perform_times_until_and_varying_repeat_exactly() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. OUTPERF. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 9 VALUE 0. 01 J PIC 9 VALUE 0. PROCEDURE DIVISION. PERFORM TIMES-P 3 TIMES. PERFORM UNTIL-P UNTIL I > 5. PERFORM VARY-P VARYING J FROM 1 BY 1 UNTIL J > 3. DISPLAY I. STOP RUN. TIMES-P. DISPLAY 'T'. ADD 1 TO I. EXIT. UNTIL-P. DISPLAY 'U'. ADD 1 TO I. EXIT. VARY-P. DISPLAY J. EXIT.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 4096) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"T\nT\nT\nU\nU\nU\n1\n2\n3\n6\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn out_of_line_perform_repetition_survives_checkpoint_restore() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. OUTPERFCP. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 99 VALUE 0. PROCEDURE DIVISION. PERFORM WORK-P 5 TIMES. DISPLAY I. STOP RUN. WORK-P. ADD 1 TO I. EXIT.";
        let artifact = compile(source).unwrap();
        let invocation = invocation(&artifact, 1024);
        let mut original = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        assert_eq!(
            original.drive(MachineResume::Start, Quantum::new(6, 4096).unwrap()),
            MachineDrive::Continue
        );
        let checkpoint = original.checkpoint().unwrap();
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        let original = drive_to_terminal(&mut original);
        let restored = drive_to_terminal(&mut restored);
        assert_eq!(original, restored);
        assert!(matches!(
            restored,
            MachineDrive::Completed(done) if done.output.bytes() == b"05\n"
        ));
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
    fn exit_perform_transfers_after_the_enclosing_inline_loop() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. EXITPERF. DATA DIVISION. WORKING-STORAGE SECTION. 01 I PIC 9 VALUE 0. PROCEDURE DIVISION.\nPERFORM VARYING I FROM 1 BY 1 UNTIL I > 4\n IF I = 3\n  EXIT PERFORM\n END-IF\n DISPLAY I\nEND-PERFORM.\nDISPLAY 'DONE'.\nSTOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"1\n2\nDONE\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn exit_paragraph_returns_from_an_out_of_line_perform() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. EXITPARA. PROCEDURE DIVISION. PERFORM WORK-P. DISPLAY 'DONE'. STOP RUN. WORK-P. DISPLAY 'WORK'. EXIT PARAGRAPH. DISPLAY 'BAD'. NEXT-P. DISPLAY 'WRONG'.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"WORK\nDONE\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn exit_section_skips_remaining_paragraphs_until_the_next_section() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. EXITSECT. PROCEDURE DIVISION. GO TO WORK-P. FIRST-S SECTION. WORK-P. DISPLAY 'WORK'. EXIT SECTION. DISPLAY 'BAD'. LATE-P. DISPLAY 'LATE'. SECOND-S SECTION. DISPLAY 'NEXT'. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"WORK\nNEXT\n"),
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
    fn set_up_and_down_by_update_all_numeric_targets() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SETDELTA. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 10. 01 B PIC 99 VALUE 20. PROCEDURE DIVISION. SET A B UP BY 3. SET A B DOWN BY 1. DISPLAY A B. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"1222\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn set_to_applies_one_value_or_address_to_every_target() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SETMULTI. DATA DIVISION. WORKING-STORAGE SECTION. 01 A PIC 99 VALUE 1. 01 B PIC 99 VALUE 2. 01 ITEM-X PIC X VALUE 'X'. 01 P POINTER. 01 Q POINTER. PROCEDURE DIVISION. SET A B TO 7. SET P Q TO ADDRESS OF ITEM-X. SET P Q TO NULL. DISPLAY A B. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"0707\n"),
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
        assert!(matches!(
            execute(&artifact, 1024),
            MachineDrive::Condition(condition) if condition.name == "SIZE-ERROR"
        ));
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
    fn national_length_and_utf8_index_width_boundaries_are_exact() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. UNICODEFN. DATA DIVISION. WORKING-STORAGE SECTION. 01 NAT-X PIC N(3) NATIONAL. 01 UTF-X PIC U(4) BYTE-LENGTH 16 UTF-8. 01 L1 PIC 99. 01 L2 PIC 99. 01 L3 PIC 99. 01 L4 PIC 99. 01 L5 PIC 99. 01 L6 PIC 99. 01 L7 PIC 99. 01 L8 PIC 99. PROCEDURE DIVISION. MOVE 'Aé🙂' TO UTF-X. MOVE FUNCTION LENGTH(NAT-X) TO L1. MOVE FUNCTION BYTE-LENGTH(NAT-X) TO L2. MOVE FUNCTION LENGTH(UTF-X) TO L3. MOVE FUNCTION ULENGTH(UTF-X, 2, 2) TO L4. MOVE FUNCTION UPOS(UTF-X, 3) TO L5. MOVE FUNCTION UWIDTH(UTF-X, 2) TO L6. MOVE FUNCTION UWIDTH(UTF-X, 3) TO L7. MOVE FUNCTION USUPPLEMENTARY(UTF-X) TO L8. DISPLAY L1 L2 L3 L4 L5 L6 L7 L8. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"0306040104020403\n")
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn test_numval_variants_return_error_positions_and_honor_currency_arguments() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. TESTNUM. DATA DIVISION. WORKING-STORAGE SECTION. 01 BAD-X PIC X(4) VALUE '12A3'. 01 CUR-X PIC X(6) VALUE '€12X'. 01 SYMBOL-X PIC X(3) VALUE '€'. 01 POS-X PIC 9. 01 VALUE-X PIC 99V9. PROCEDURE DIVISION. MOVE FUNCTION TEST-NUMVAL(BAD-X) TO POS-X. DISPLAY POS-X. MOVE FUNCTION TEST-NUMVAL-C(CUR-X, SYMBOL-X) TO POS-X. DISPLAY POS-X. MOVE FUNCTION NUMVAL-C('€12.5', SYMBOL-X) TO VALUE-X. DISPLAY VALUE-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        match execute(&artifact, 1024) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"3\n4\n125\n"),
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
        let dynamic_invocation = invocation(&artifact, 1024);
        let mut first = ReferenceMachine::from_binary(
            artifact.payload(),
            dynamic_invocation.clone(),
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
            "mainframe-env.reference-machine-checkpoint@10"
        );
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            dynamic_invocation,
            CodecLimits::default(),
        )
        .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(
            drive_to_terminal(&mut first),
            drive_to_terminal(&mut restored)
        );
    }

    #[test]
    fn checkpoint_v10_preserves_dynamic_sort_and_allocated_linkage_state() {
        let dynamic_source = "IDENTIFICATION DIVISION. PROGRAM-ID. DYNCP. DATA DIVISION. WORKING-STORAGE SECTION. 01 DYN-X PIC X DYNAMIC LENGTH LIMIT IS 8. PROCEDURE DIVISION. MOVE 'HELLO' TO DYN-X. DISPLAY DYN-X. STOP RUN.";
        let artifact = compile(dynamic_source).unwrap();
        let dynamic_invocation = invocation(&artifact, 1024);
        let mut first = ReferenceMachine::from_binary(
            artifact.payload(),
            dynamic_invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        while first
            .variable("DYN-X")
            .is_none_or(|value| value.bytes() != b"HELLO")
        {
            assert_eq!(
                first.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
                MachineDrive::Continue
            );
        }
        let checkpoint = first.checkpoint().unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            dynamic_invocation,
            CodecLimits::default(),
        )
        .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.variable("DYN-X").unwrap().bytes(), b"HELLO");
        assert_eq!(
            drive_to_terminal(&mut first),
            drive_to_terminal(&mut restored)
        );

        let sort_source = "IDENTIFICATION DIVISION. PROGRAM-ID. SORTCP. DATA DIVISION. FILE SECTION. SD SORT-FILE. 01 SORT-RECORD. 05 SORT-KEY PIC X(2). 05 SORT-DATA PIC X(2). WORKING-STORAGE SECTION. 01 OUT-X PIC X(4). PROCEDURE DIVISION. SORT SORT-FILE ON ASCENDING KEY SORT-KEY INPUT PROCEDURE FEED OUTPUT PROCEDURE DRAIN. STOP RUN. FEED. MOVE 'BB02' TO SORT-RECORD. RELEASE SORT-RECORD. MOVE 'AA01' TO SORT-RECORD. RELEASE SORT-RECORD. EXIT. DRAIN. RETURN SORT-FILE RECORD INTO OUT-X. DISPLAY OUT-X. RETURN SORT-FILE RECORD INTO OUT-X. DISPLAY OUT-X. EXIT.";
        let artifact = compile(sort_source).unwrap();
        let sort_invocation = invocation(&artifact, 1024);
        let mut first = ReferenceMachine::from_binary(
            artifact.payload(),
            sort_invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        while !first.position_summary().contains("move [\"'AA01'\"") {
            assert_eq!(
                first.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
                MachineDrive::Continue
            );
        }
        let checkpoint = first.checkpoint().unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            sort_invocation,
            CodecLimits::default(),
        )
        .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        let expected = drive_to_terminal(&mut first);
        assert_eq!(expected, drive_to_terminal(&mut restored));
        assert!(
            matches!(expected, MachineDrive::Completed(done) if done.output.bytes() == b"AA01\nBB02\n")
        );

        let heap_source = "IDENTIFICATION DIVISION. PROGRAM-ID. HEAPCP. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR POINTER. LINKAGE SECTION. 01 BLOCK PIC X(4). PROCEDURE DIVISION. ALLOCATE 4 CHARACTERS RETURNING PTR. SET ADDRESS OF BLOCK TO PTR. MOVE 'HEAP' TO BLOCK. DISPLAY BLOCK. FREE PTR. STOP RUN.";
        let artifact = compile(heap_source).unwrap();
        let invocation = invocation(&artifact, 1024);
        let mut first = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        while first
            .variable("BLOCK")
            .is_none_or(|value| value.bytes() != b"HEAP")
        {
            assert_eq!(
                first.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
                MachineDrive::Continue
            );
        }
        let checkpoint = first.checkpoint().unwrap();
        assert_eq!(
            checkpoint.schema(),
            "mainframe-env.reference-machine-checkpoint@10"
        );
        let mut restored =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.variable("BLOCK").unwrap().bytes(), b"HEAP");
        let expected = drive_to_terminal(&mut first);
        assert_eq!(expected, drive_to_terminal(&mut restored));
        assert!(
            matches!(expected, MachineDrive::Completed(done) if done.output.bytes() == b"HEAP\n")
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
        let HostRequest::Program(ProgramRequest::Call {
            program, payload, ..
        }) = &effect.request
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
    fn entry_binding_selects_the_alternate_entry_after_storage_initialization() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. ENTRYPOINT. DATA DIVISION. WORKING-STORAGE SECTION. 01 VALUE-X PIC X(4) VALUE 'INIT'. PROCEDURE DIVISION. DISPLAY 'MAIN'. STOP RUN. ENTRY 'ALT'. DISPLAY VALUE-X. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut invocation = invocation(&artifact, 1024);
        invocation.bindings.insert(
            "cobol.entry".into(),
            mainframe_env_execution_api::BoundedPayload::new(
                "mainframe-env.cobol-entry@1",
                b"ALT".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        match drive_to_terminal(&mut machine) {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"INIT\n"),
            other => panic!("{other:?}"),
        }
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
    fn read_lock_and_wait_phrases_survive_in_the_typed_dataset_effect() {
        use mainframe_env_host_api::{
            DatasetReadControl, DatasetReadLockMode, DatasetRequest, HostRequest,
        };

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. READCONTROL. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED ACCESS MODE IS RANDOM RECORD KEY IS REC-KEY. DATA DIVISION. FILE SECTION. FD TEST-FILE. 01 TEST-REC. 05 REC-KEY PIC X(2) VALUE 'AA'. 05 DATA-X PIC X(2). PROCEDURE DIVISION. READ TEST-FILE RECORD KEY IS REC-KEY WITH KEPT LOCK NO WAIT. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let MachineDrive::HostCall(effect) =
            machine.drive(MachineResume::Start, Quantum::new(64, 4096).unwrap())
        else {
            panic!("READ did not emit a host effect");
        };
        assert!(matches!(
            effect.request,
            HostRequest::Dataset(DatasetRequest::Read {
                control: DatasetReadControl {
                    lock: DatasetReadLockMode::KeptLock,
                    wait: Some(false),
                },
                ..
            })
        ));
    }

    #[test]
    fn open_and_close_apply_every_file_and_retain_each_close_phrase() {
        use mainframe_env_host_api::{DatasetRequest, HostRequest};

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MULTIFILE. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT FIRST-FILE ASSIGN TO FIRSTDD ORGANIZATION IS SEQUENTIAL. SELECT SECOND-FILE ASSIGN TO SECONDDD ORGANIZATION IS SEQUENTIAL. DATA DIVISION. FILE SECTION. FD FIRST-FILE. 01 FIRST-REC PIC X. FD SECOND-FILE. 01 SECOND-REC PIC X. PROCEDURE DIVISION. OPEN OUTPUT FIRST-FILE SECOND-FILE. CLOSE FIRST-FILE WITH LOCK SECOND-FILE. DISPLAY 'DONE'. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let mut resume = MachineResume::Start;
        let mut requests = Vec::new();
        loop {
            match machine.drive(resume, Quantum::new(256, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    let HostRequest::Dataset(request) = &effect.request else {
                        panic!("unexpected effect: {:?}", effect.request);
                    };
                    let (kind, dataset) = match request {
                        DatasetRequest::Truncate { dataset, .. } => ("truncate", dataset.as_str()),
                        DatasetRequest::Close { dataset, .. } => ("close", dataset.as_str()),
                        other => panic!("unexpected dataset request: {other:?}"),
                    };
                    requests.push(format!("{kind}:{dataset}"));
                    resume = MachineResume::HostResult(
                        crate::cobol_runtime::effect_result(&effect).unwrap(),
                    );
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(done.output.bytes(), b"DONE\n");
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            requests,
            [
                "truncate:FIRSTDD",
                "truncate:SECONDDD",
                "close:FIRSTDD",
                "close:SECONDDD",
            ]
        );
    }
    #[test]
    fn write_advancing_updates_linage_and_selects_end_of_page_branches() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. WRITEPAGE. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT PRINT-FILE ASSIGN TO PRINTDD ORGANIZATION IS SEQUENTIAL. DATA DIVISION. FILE SECTION. FD PRINT-FILE LINAGE IS 3 LINES. 01 PRINT-REC PIC X. WORKING-STORAGE SECTION. 01 VALUE-X PIC X VALUE 'A'. PROCEDURE DIVISION. WRITE PRINT-REC FROM VALUE-X AFTER ADVANCING 2 LINES AT END-OF-PAGE DISPLAY 'EOP' NOT AT END-OF-PAGE DISPLAY 'BAD1' END-WRITE. WRITE PRINT-REC FROM VALUE-X BEFORE ADVANCING PAGE AT END-OF-PAGE DISPLAY 'PAGE' NOT AT END-OF-PAGE DISPLAY 'BAD2' END-WRITE. DISPLAY LINAGE-COUNTER. STOP RUN.";
        let artifact = compile(source).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation(&artifact, 1024),
            CodecLimits::default(),
        )
        .unwrap();
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(resume, Quantum::new(256, 4096).unwrap()) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    resume = MachineResume::HostResult(
                        crate::cobol_runtime::effect_result(&effect).unwrap(),
                    );
                }
                MachineDrive::Completed(done) => {
                    assert_eq!(done.output.bytes(), b"EOP\nPAGE\n1\n");
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
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

        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CICSRESULT. DATA DIVISION. WORKING-STORAGE SECTION. 01 DATA-X PIC X(3). 01 RESP-X PIC 99. 01 RESP2-X PIC 99. 01 EIBRESP PIC 99. 01 EIBRESP2 PIC 99. 01 EIBCALEN PIC 99. 01 EIBAID PIC X. 01 EIBTRNID PIC X(4). PROCEDURE DIVISION. EXEC CICS READ DATASET('D') RIDFLD('K') INTO(DATA-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. DISPLAY DATA-X. DISPLAY RESP-X. DISPLAY RESP2-X. DISPLAY EIBCALEN. DISPLAY EIBTRNID. STOP RUN.";
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
