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
fn compiled_issue_endfile_selects_its_3740_host_operation_and_endoutput_flag() {
    let source = b"IDENTIFICATION DIVISION. PROGRAM-ID. ISSUEEF. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS ISSUE ENDFILE ENDOUTPUT RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
    let source_limits = SourceLimits::default();
    let path = LogicalPath::new("ISSUEEF.cbl", source_limits.max_path_bytes).unwrap();
    let file = SourceFile::input(
        "ISSUEEF.cbl",
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
        panic!("ISSUE ENDFILE did not publish");
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
        .unwrap_or_else(|| panic!("typed ISSUE ENDFILE plan: {module:?}"));
    assert_eq!(plan.operation, CicsPlanOperation::IssueEndfile);
    assert!(plan.options.contains(&CicsPlanOption::IssueEndOutput));

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
            other => panic!("ISSUE ENDFILE did not dispatch: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = effect.request else {
        panic!("ISSUE ENDFILE selected a non-CICS host request");
    };
    assert_eq!(request.operation, CicsOperation::IssueEndfile);
    assert!(request.arguments.contains_key("OPTION.ENDOUTPUT"));
    assert!(request.arguments.contains_key("RESP"));
    assert!(request.arguments.contains_key("RESP2"));
}
