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
use mainframe_env_host_api::{CicsOperation, HostRequest};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::{
    Attribute, CicsPlanLimits, CicsPlanOperation, CodecLimits, decode_binary,
    decode_cics_effect_plan,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn compiled_wait_signal_preserves_distinct_plan_and_selected_host_identity() {
    let source = b"IDENTIFICATION DIVISION. PROGRAM-ID. SIGWAIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS WAIT SIGNAL RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
    let source_limits = SourceLimits::default();
    let path = LogicalPath::new("SIGWAIT.cbl", source_limits.max_path_bytes).unwrap();
    let file = SourceFile::input(
        "SIGWAIT.cbl",
        source.to_vec(),
        SourceFormat::Free,
        SourceEncoding::Utf8,
        source_limits,
    )
    .unwrap();
    let bundle = SourceBundle::new(
        &path,
        vec![file],
        BTreeMap::new(),
        Vec::new(),
        source_limits,
    )
    .unwrap();
    let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap()
    else {
        panic!("WAIT SIGNAL did not publish");
    };
    let binary = artifact.payload();
    let module = decode_binary(binary, CodecLimits::default()).unwrap();
    let plan = module
        .regions()
        .iter()
        .flat_map(|region| region.blocks.iter())
        .flat_map(|block| block.operations.iter())
        .find_map(|operation| match operation.attributes.get("cics_plan") {
            Some(Attribute::Bytes(bytes)) => {
                decode_cics_effect_plan(bytes, CicsPlanLimits::default())
                    .ok()
                    .filter(|plan| plan.operation == CicsPlanOperation::WaitSignal)
            }
            _ => None,
        })
        .expect("typed WAIT SIGNAL plan");
    assert!(plan.operands.is_empty());
    let limits = InvocationLimits::default();
    let invocation = Invocation::new(
        RequestId::new("wait-request", limits).unwrap(),
        ExecutionId::new("wait-execution", limits).unwrap(),
        RunUnitId::new("wait-run", limits).unwrap(),
        None,
        Selector::new("program:SIGWAIT", limits).unwrap(),
        ArtifactRef::new("wait-artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("wait-trace", limits).unwrap(),
        IdempotencyKey::new("wait-invocation", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap();
    let mut machine =
        ReferenceMachine::from_binary(binary, invocation, CodecLimits::default()).unwrap();
    let effect = loop {
        match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("WAIT SIGNAL did not dispatch: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = effect.request else {
        panic!("WAIT SIGNAL used the wrong host request");
    };
    assert_eq!(request.operation, CicsOperation::WaitSignal);
    assert!(
        request
            .arguments
            .keys()
            .all(|name| matches!(name.as_str(), "RESP" | "RESP2"))
    );
}

#[test]
fn compiled_mapped_data_wait_routes_keep_their_plan_and_host_identity() {
    let cases = [
        (
            "RECEIVE CONVID(CV-X) INTO(IN-X) LENGTH(LEN-X) MAXLENGTH(8) STATE(STATE-X)",
            CicsPlanOperation::ReceiveConversation,
            CicsOperation::ReceiveConversation,
        ),
        (
            "SEND CONVID(CV-X) FROM(DATA-X) LENGTH(5) WAIT STATE(STATE-X)",
            CicsPlanOperation::SendConversation,
            CicsOperation::SendConversation,
        ),
        (
            "WAIT CONVID(CV-X) STATE(STATE-X)",
            CicsPlanOperation::WaitConvid,
            CicsOperation::WaitConvid,
        ),
        (
            "WAIT TERMINAL CONVID(CV-X)",
            CicsPlanOperation::WaitTerminal,
            CicsOperation::WaitTerminal,
        ),
    ];
    for (command, expected_plan, expected_host) in cases {
        let source = format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CVROUTE. DATA DIVISION. WORKING-STORAGE SECTION. 01 CV-X PIC X(4) VALUE 'TEST'. 01 DATA-X PIC X(8) VALUE 'HELLO'. 01 IN-X PIC X(8). 01 LEN-X PIC S9(4) COMP VALUE 8. 01 STATE-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS {command} END-EXEC. STOP RUN."
        );
        let source_limits = SourceLimits::default();
        let path = LogicalPath::new("CVROUTE.cbl", source_limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "CVROUTE.cbl",
            source.into_bytes(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            source_limits,
        )
        .unwrap();
        let bundle = SourceBundle::new(
            &path,
            vec![file],
            BTreeMap::new(),
            Vec::new(),
            source_limits,
        )
        .unwrap();
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("{command} did not publish");
        };
        let binary = artifact.payload();
        let module = decode_binary(binary, CodecLimits::default()).unwrap();
        let plan = module
            .regions()
            .iter()
            .flat_map(|region| region.blocks.iter())
            .flat_map(|block| block.operations.iter())
            .find_map(|operation| match operation.attributes.get("cics_plan") {
                Some(Attribute::Bytes(bytes)) => {
                    decode_cics_effect_plan(bytes, CicsPlanLimits::default()).ok()
                }
                _ => None,
            })
            .expect("typed data/wait plan");
        assert_eq!(plan.operation, expected_plan, "{command}");
        let limits = InvocationLimits::default();
        let invocation = Invocation::new(
            RequestId::new("route-request", limits).unwrap(),
            ExecutionId::new("route-execution", limits).unwrap(),
            RunUnitId::new("route-run", limits).unwrap(),
            None,
            Selector::new("program:CVROUTE", limits).unwrap(),
            ArtifactRef::new("route-artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("route-trace", limits).unwrap(),
            IdempotencyKey::new("route-invocation", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap();
        let mut machine =
            ReferenceMachine::from_binary(binary, invocation, CodecLimits::default()).unwrap();
        let effect = loop {
            match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
                MachineDrive::Continue => continue,
                MachineDrive::HostCall(effect) => break effect,
                other => panic!("{command} did not dispatch: {other:?}"),
            }
        };
        let HostRequest::Cics(request) = effect.request else {
            panic!("{command} selected a non-CICS host request");
        };
        assert_eq!(request.operation, expected_host, "{command}");
    }
}
