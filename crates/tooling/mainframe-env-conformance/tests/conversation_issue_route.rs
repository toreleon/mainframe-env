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
    Attribute, CicsPlanLimits, CicsPlanOperation, CicsPlanOption, CodecLimits, decode_binary,
    decode_cics_effect_plan,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn compiled_issue_device_markers_select_exact_host_operations_and_companion_flags() {
    for (command, plan, host, flag, operand) in [
        (
            "ISSUE ENDFILE ENDOUTPUT",
            CicsPlanOperation::IssueEndfile,
            CicsOperation::IssueEndfile,
            Some((CicsPlanOption::IssueEndOutput, "OPTION.ENDOUTPUT")),
            None,
        ),
        (
            "ISSUE ENDOUTPUT ENDFILE",
            CicsPlanOperation::IssueEndoutput,
            CicsOperation::IssueEndoutput,
            Some((CicsPlanOption::IssueEndFile, "OPTION.ENDFILE")),
            None,
        ),
        (
            "ISSUE EODS",
            CicsPlanOperation::IssueEods,
            CicsOperation::IssueEods,
            None,
            None,
        ),
        (
            "ISSUE ERASEAUP WAIT",
            CicsPlanOperation::IssueEraseAup,
            CicsOperation::IssueEraseAup,
            Some((CicsPlanOption::IssueWaitOption, "OPTION.WAIT")),
            None,
        ),
        (
            "ISSUE DISCONNECT SESSION(SS-X)",
            CicsPlanOperation::IssueDisconnect,
            CicsOperation::IssueDisconnect,
            None,
            Some("SESSION"),
        ),
        (
            "ISSUE RESET",
            CicsPlanOperation::IssueReset,
            CicsOperation::IssueReset,
            None,
            None,
        ),
        (
            "ISSUE LOAD PROGRAM('APP1') CONVERSE",
            CicsPlanOperation::IssueLoad,
            CicsOperation::IssueLoad,
            Some((CicsPlanOption::IssueConverse, "OPTION.CONVERSE")),
            Some("PROGRAM"),
        ),
    ] {
        compiled_case(command, plan, host, flag, operand);
    }
}

fn compiled_case(
    command: &str,
    expected_plan: CicsPlanOperation,
    expected_host: CicsOperation,
    flag: Option<(CicsPlanOption, &str)>,
    operand: Option<&str>,
) {
    let source = format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. ISSUEEF. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. 01 SS-X PIC X(4) VALUE 'S001'. PROCEDURE DIVISION. EXEC CICS {command} RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN."
    );
    let source_limits = SourceLimits::default();
    let path = LogicalPath::new("ISSUEEF.cbl", source_limits.max_path_bytes).unwrap();
    let file = SourceFile::input(
        "ISSUEEF.cbl",
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
            Some(Attribute::Bytes(bytes)) => Some(
                decode_cics_effect_plan(bytes, CicsPlanLimits::default())
                    .unwrap_or_else(|error| panic!("ISSUE plan decode: {error:?}")),
            ),
            _ => None,
        })
        .unwrap_or_else(|| panic!("typed {command} plan: {module:?}"));
    assert_eq!(plan.operation, expected_plan);
    assert_eq!(plan.options.len(), usize::from(flag.is_some()));
    if let Some((option, _)) = flag {
        assert!(plan.options.contains(&option));
    }

    let limits = InvocationLimits::default();
    let invocation = Invocation::new(
        RequestId::new("issue-request", limits).unwrap(),
        ExecutionId::new("issue-execution", limits).unwrap(),
        RunUnitId::new("issue-run", limits).unwrap(),
        None,
        Selector::new("program:ISSUEEF", limits).unwrap(),
        ArtifactRef::new("issue-artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("issue-trace", limits).unwrap(),
        IdempotencyKey::new("issue-invocation", limits).unwrap(),
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
    assert_eq!(request.operation, expected_host);
    if let Some((_, name)) = flag {
        assert!(request.arguments.contains_key(name));
    }
    if let Some(name) = operand {
        assert!(request.arguments.contains_key(name));
    }
    assert!(request.arguments.contains_key("RESP"));
    assert!(request.arguments.contains_key("RESP2"));
}
