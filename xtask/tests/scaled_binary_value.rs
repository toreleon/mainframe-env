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
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};

fn run(source: &str) -> String {
    let limits = SourceLimits::default();
    let path = LogicalPath::new("SCALE.cbl", limits.max_path_bytes).unwrap();
    let file = SourceFile::input(
        "SCALE.cbl",
        source.as_bytes().to_vec(),
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .unwrap();
    let bundle = SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap();
    let CompilerResult::Published { artifact, .. } = result else {
        panic!("compilation failed: {result:?}");
    };
    let limits = InvocationLimits::default();
    let invocation = Invocation::new(
        RequestId::new("scaled-binary-request", limits).unwrap(),
        ExecutionId::new("scaled-binary-execution", limits).unwrap(),
        RunUnitId::new("scaled-binary-run", limits).unwrap(),
        None,
        Selector::new("program:SCALE", limits).unwrap(),
        ArtifactRef::new(artifact.content_id().to_reference(), limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("scaled-binary-trace", limits).unwrap(),
        IdempotencyKey::new("scaled-binary-idempotency", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap();
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .unwrap();
    match machine.drive(MachineResume::Start, Quantum::new(1000, 65_536).unwrap()) {
        MachineDrive::Completed(done) => String::from_utf8(done.output.bytes().to_vec()).unwrap(),
        other => panic!("execution failed: {other:?}"),
    }
}

#[test]
fn scaled_binary_value_move_and_arithmetic_match_cobc_ibm_std_truncation() {
    // Values and rounded DIVIDE INTO were checked with GnuCOBOL 3.2
    // -std=ibm -fbinary-truncate.
    for usage in ["COMP", "COMP-4", "BINARY", "COMP-5"] {
        for (fraction, value, coefficient, factor) in [
            ("9", "95.8", 958i128, 10i128),
            ("99", "95.85", 9585, 100),
            ("9(4)", "95.8125", 958125, 10_000),
        ] {
            for signed in [false, true] {
                let sign = if signed { -1 } else { 1 };
                let picture = if signed {
                    format!("S9(7)V{fraction}")
                } else {
                    format!("9(7)V{fraction}")
                };
                let value = if signed {
                    format!("-{value}")
                } else {
                    value.to_string()
                };
                let source = format!(
                    "identification division. program-id. SCALE.\n\
                     data division. working-storage section.\n\
                     01 R pic {picture} usage {usage} value {value}.\n\
                     01 OUT-NUM pic s9(7)v9(4) sign trailing separate.\n\
                     procedure division.\n\
                     move R to OUT-NUM. display OUT-NUM.\n\
                     move {value} to R. move R to OUT-NUM. display OUT-NUM.\n\
                     move {value} to R. add 2 to R rounded. move R to OUT-NUM. display OUT-NUM.\n\
                     move {value} to R. subtract 2 from R. move R to OUT-NUM. display OUT-NUM.\n\
                     move {value} to R. multiply 2 by R. move R to OUT-NUM. display OUT-NUM.\n\
                     move {value} to R. divide 2 into R rounded. move R to OUT-NUM. display OUT-NUM.\n\
                     move {value} to R. compute R = R + 2. move R to OUT-NUM. display OUT-NUM.\n\
                     goback.\n"
                );
                let base = sign * coefficient;
                let rounded_half = sign * ((coefficient + 1) / 2);
                let expected = [
                    base,
                    base,
                    base + 2 * factor,
                    base - 2 * factor,
                    base * 2,
                    rounded_half,
                    base + 2 * factor,
                ]
                .map(|coefficient| {
                    let sign = if coefficient < 0 { '-' } else { '+' };
                    format!("{:011}{sign}\n", coefficient.abs() * (10_000 / factor))
                })
                .concat();
                assert_eq!(run(&source), expected, "{usage} {picture} VALUE {value}");
            }
        }
    }
}

#[test]
fn scaled_binary_arithmetic_truncates_std_to_picture_but_not_comp5() {
    // GnuCOBOL 3.2 -std=ibm -fbinary-truncate stores 9.70 in ordinary binary
    // PIC 9V99, while COMP-5 retains 19.70.
    for usage in ["COMP", "COMP-4", "BINARY", "COMP-5"] {
        for signed in [false, true] {
            let picture = if signed { "S9V99" } else { "9V99" };
            let value = if signed { "-9.85" } else { "9.85" };
            let sign = if signed { '-' } else { '+' };
            let source = format!(
                "identification division. program-id. SCALE.\n\
                 data division. working-storage section.\n\
                 01 R pic {picture} usage {usage} value {value}.\n\
                 01 OUT-NUM pic s9(7)v99 sign trailing separate.\n\
                 procedure division.\n\
                 move R to OUT-NUM. display OUT-NUM.\n\
                 multiply 2 by R. move R to OUT-NUM. display OUT-NUM.\n\
                 goback.\n"
            );
            let multiplied = if usage == "COMP-5" {
                "000001970"
            } else {
                "000000970"
            };
            assert_eq!(
                run(&source),
                format!("000000985{sign}\n{multiplied}{sign}\n"),
                "{usage} {picture}"
            );
        }
    }
}

#[test]
fn binary_std_truncates_arithmetic_to_picture_and_preserves_size_error_receiver() {
    for usage in ["COMP", "COMP-4", "BINARY"] {
        let source = format!(
            "identification division. program-id. SCALE.\n\
             data division. working-storage section.\n\
             01 R pic 9(2) usage {usage} value 99.\n\
             01 C pic 9(2) usage {usage} value 0.\n\
             01 OUT-NUM pic 9(4).\n\
             procedure division.\n\
             add 1 to R. move R to OUT-NUM. display OUT-NUM.\n\
             move 99 to R.\n\
             add 1 to R on size error display 'SIZE' end-add.\n\
             move R to OUT-NUM. display OUT-NUM.\n\
             compute C = 150. move C to OUT-NUM. display OUT-NUM.\n\
             move 150 to R. move R to OUT-NUM. display OUT-NUM.\n\
             goback.\n"
        );
        assert_eq!(run(&source), "0000\nSIZE\n0099\n0050\n0050\n", "{usage}");
    }
}

#[test]
fn comp5_uses_full_native_binary_range() {
    let source = "identification division. program-id. SCALE.\n\
        data division. working-storage section.\n\
        01 R pic 9(2) usage comp-5 value 99.\n\
        01 FULLWORD pic 9(9) usage comp-5 value 4294967295.\n\
        01 DOUBLEWORD pic 9(18) usage comp-5 value 18446744073709551615.\n\
        01 OUT-NUM pic 9(20).\n\
        procedure division.\n\
        add 1 to R. move R to OUT-NUM. display OUT-NUM.\n\
        move 40000 to R. move R to OUT-NUM. display OUT-NUM.\n\
        move FULLWORD to OUT-NUM. display OUT-NUM.\n\
        move DOUBLEWORD to OUT-NUM. display OUT-NUM.\n\
        goback.\n";
    assert_eq!(
        run(source),
        "00000000000000000100\n00000000000000040000\n00000000004294967295\n18446744073709551615\n"
    );
}
