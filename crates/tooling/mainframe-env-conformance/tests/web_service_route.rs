use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits, Machine,
    MachineDrive, MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits,
    RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{CicsOperation, HostRequest};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::{CicsPlanLimits, CodecLimits, decode_binary, decode_cics_effect_plan};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};

fn compile(source: &str) -> Vec<u8> {
    let limits = SourceLimits::default();
    let path = LogicalPath::new("WEBCTRL.cbl", limits.max_path_bytes).unwrap();
    let file = SourceFile::input(
        "WEBCTRL.cbl",
        source.as_bytes().to_vec(),
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .unwrap();
    let source = SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap();
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap();
    let CompilerResult::Published { artifact, .. } = result else {
        panic!("not published: {result:?}");
    };
    artifact.payload().to_vec()
}

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("web-req", limits).unwrap(),
        ExecutionId::new("web-exec", limits).unwrap(),
        RunUnitId::new("web-run", limits).unwrap(),
        None,
        Selector::new("program:WEBCTRL", limits).unwrap(),
        ArtifactRef::new("web-artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::<CapabilityId>::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("web-trace", limits).unwrap(),
        IdempotencyKey::new("web-idem", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

#[test]
fn compiled_wsaepr_create_has_typed_plan_and_selected_host_route() {
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. WEBCTRL. DATA DIVISION. WORKING-STORAGE SECTION. 01 ADDR-X PIC X(255) VALUE 'http://example.invalid/service'. 01 EPR-X PIC X(512). 01 EPR-LEN PIC S9(9) COMP VALUE 512. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS WSAEPR CREATE ADDRESS(ADDR-X) EPRINTO(EPR-X) EPRLENGTH(EPR-LEN) RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
    let binary = compile(source);
    let module = decode_binary(&binary, CodecLimits::default()).unwrap();
    let plan = module
        .regions()
        .iter()
        .flat_map(|region| region.blocks.iter())
        .flat_map(|block| block.operations.iter())
        .find_map(|operation| match operation.attributes.get("cics_plan") {
            Some(mainframe_env_ir::Attribute::Bytes(bytes)) => {
                decode_cics_effect_plan(bytes, CicsPlanLimits::default())
                    .ok()
                    .filter(|plan| {
                        plan.operation == mainframe_env_ir::CicsPlanOperation::WsaEprCreate
                    })
            }
            _ => None,
        })
        .expect("compiled WSAEPR CREATE plan");
    assert_eq!(
        plan.outputs
            .iter()
            .filter(|output| matches!(
                output.name,
                mainframe_env_ir::CicsOutputName::WebEprInto
                    | mainframe_env_ir::CicsOutputName::WebEprLength
            ))
            .count(),
        2
    );
    let mut machine =
        ReferenceMachine::from_binary(&binary, invocation(), CodecLimits::default()).unwrap();
    let effect = loop {
        match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("web route did not dispatch: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = effect.request else {
        panic!("wrong host request");
    };
    assert_eq!(request.operation, CicsOperation::WsaEprCreate);
    assert_eq!(request.arguments["ADDRESS"].bytes().len(), 255);
    assert_eq!(request.arguments["EPRINTO.MAXLENGTH"].bytes(), b"512");
    assert_eq!(request.arguments["EPRLENGTH"].bytes(), b"512");
}

#[test]
fn compiled_invoke_service_keeps_service_channel_and_operation_distinct() {
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. WEBCTRL. DATA DIVISION. WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. 01 RESP2-X PIC S9(9) COMP. PROCEDURE DIVISION. EXEC CICS INVOKE SERVICE('WEBSVC') CHANNEL('WEBCHAN') OPERATION('FETCH') RESP(RESP-X) RESP2(RESP2-X) END-EXEC. STOP RUN.";
    let binary = compile(source);
    let mut machine =
        ReferenceMachine::from_binary(&binary, invocation(), CodecLimits::default()).unwrap();
    let effect = loop {
        match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("invoke service did not dispatch: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = effect.request else {
        panic!("wrong host request");
    };
    assert_eq!(request.operation, CicsOperation::InvokeService);
    assert_eq!(request.arguments["SERVICE"].bytes(), b"WEBSVC");
    assert_eq!(request.arguments["CHANNEL"].bytes(), b"WEBCHAN");
    assert_eq!(request.arguments["OPERATION"].bytes(), b"FETCH");
}

#[test]
fn compiled_eprset_pointer_survives_checkpoint_with_owned_bytes() {
    use mainframe_env_execution_api::BoundedPayload;
    use mainframe_env_host_api::{CicsDisposition, CicsResponse, EffectResult, HostResult};
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. WEBCTRL. DATA DIVISION. WORKING-STORAGE SECTION. 01 ADDR-X PIC X(255) VALUE 'http://example.invalid/service'. 01 EPR-PTR POINTER. 01 EPR-LEN PIC S9(9) COMP VALUE 512. PROCEDURE DIVISION. EXEC CICS WSAEPR CREATE ADDRESS(ADDR-X) EPRSET(EPR-PTR) EPRLENGTH(EPR-LEN) END-EXEC. STOP RUN.";
    let binary = compile(source);
    let invocation = invocation();
    let mut machine =
        ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default()).unwrap();
    let effect = loop {
        match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("EPRSET did not dispatch: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = &effect.request else {
        panic!("wrong request");
    };
    assert_eq!(request.operation, CicsOperation::WsaEprCreate);
    assert!(request.arguments["EPRSET.MAXLENGTH"].bytes() != b"0");
    let payload = |schema: &str, bytes: &[u8]| {
        BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap()
    };
    let response = CicsResponse {
        disposition: CicsDisposition::Complete,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        applid: "MEAPPL".into(),
        sysid: "MESYS".into(),
        transaction: "DEFAULT".into(),
        aid: 0,
        target: None,
        next_transaction: None,
        payload: payload("mainframe-env.cics.payload@1", b""),
        outputs: BTreeMap::from([
            (
                "EPRSET".into(),
                payload("mainframe-env.cics.payload@1", b"<epr/>"),
            ),
            (
                "EPRLENGTH".into(),
                payload("mainframe-env.cics.decimal@1", b"6"),
            ),
        ]),
        unit_of_work: None,
    };
    assert!(matches!(
        machine.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(HostResult::Cics(response)),
            }),
            Quantum::new(1, 1024).unwrap()
        ),
        MachineDrive::Continue | MachineDrive::Completed(_)
    ));
    let pointer = machine.variable("EPR-PTR").unwrap().bytes().to_vec();
    assert!(pointer.iter().any(|byte| *byte != 0));
    assert_eq!(machine.snapshot().base_storage.last().unwrap(), b"<epr/>");
    let checkpoint = machine.checkpoint().unwrap();
    let mut restored =
        ReferenceMachine::from_binary(&binary, invocation, CodecLimits::default()).unwrap();
    restored.restore_checkpoint(&checkpoint).unwrap();
    assert_eq!(restored.variable("EPR-PTR").unwrap().bytes(), pointer);
    assert_eq!(restored.snapshot().base_storage.last().unwrap(), b"<epr/>");
}

#[test]
fn compiled_wsa_context_get_selects_action_without_epr_destination() {
    let source = "IDENTIFICATION DIVISION. PROGRAM-ID. WEBCTRL. DATA DIVISION. WORKING-STORAGE SECTION. 01 ACTION-X PIC X(255). PROCEDURE DIVISION. EXEC CICS WSACONTEXT GET CHANNEL('WEBCHAN') ACTION(ACTION-X) END-EXEC. STOP RUN.";
    let binary = compile(source);
    let mut machine =
        ReferenceMachine::from_binary(&binary, invocation(), CodecLimits::default()).unwrap();
    let effect = loop {
        match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("WSACONTEXT GET did not dispatch: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = effect.request else {
        panic!("wrong host request");
    };
    assert_eq!(request.operation, CicsOperation::WsaContextGet);
    assert!(request.arguments.contains_key("ACTION"));
    assert!(!request.arguments.contains_key("EPRINTO"));
}
