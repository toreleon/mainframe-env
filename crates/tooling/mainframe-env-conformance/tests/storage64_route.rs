use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, Machine, MachineDrive, MachineResume, Principal, PrincipalId, Quantum,
    RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{CicsDisposition, CicsOperation, HostRequest, HostResult};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::{
    CicsNamedOperand, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanLimits,
    CodecLimits, ModuleBuilder, decode_binary, decode_cics_effect_plan, encode_binary,
    encode_cics_effect_plan,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use std::collections::{BTreeMap, BTreeSet};

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("req", limits).unwrap(),
        ExecutionId::new("exec", limits).unwrap(),
        RunUnitId::new("run", limits).unwrap(),
        None,
        Selector::new("program:STOR64", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::<CapabilityId>::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("trace", limits).unwrap(),
        IdempotencyKey::new("idem", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn translated_getmain64() -> Vec<u8> {
    // The COBOL frontend supplies layout and control-flow scaffolding. This
    // test adapter replaces GETMAIN with a distinct checked AMODE(64) plan.
    let source = "PROCESS LP(64)\nIDENTIFICATION DIVISION. PROGRAM-ID. STOR64. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC S9(9) COMP VALUE 17. 01 FLAG-X PIC X. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(LEN-X) END-EXEC. MOVE 'Q' TO FLAG-X. STOP RUN.";
    let source_limits = SourceLimits::default();
    let path = LogicalPath::new("STOR64.cbl", source_limits.max_path_bytes).unwrap();
    let file = SourceFile::input(
        "STOR64.cbl",
        source.as_bytes().to_vec(),
        SourceFormat::Free,
        SourceEncoding::Utf8,
        source_limits,
    )
    .unwrap();
    let source = SourceBundle::new(
        &path,
        vec![file],
        BTreeMap::new(),
        Vec::new(),
        source_limits,
    )
    .unwrap();
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap();
    let CompilerResult::Published { artifact, .. } = result else {
        panic!("expected compiled scaffold: {result:?}");
    };
    let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
    let mut builder = ModuleBuilder::new(mainframe_env_ir::IrLimits::default());
    for storage in module.storage() {
        assert_eq!(
            builder
                .add_storage(&storage.name, storage.size, storage.alias_of.clone())
                .unwrap(),
            storage.id,
        );
    }
    let mut selected = 0;
    for region in module.regions() {
        let new_region = builder.add_region().unwrap();
        for block in &region.blocks {
            let new_block = builder.add_block(new_region).unwrap();
            for operation in &block.operations {
                let mut identity = operation.identity.clone();
                let mut attributes = operation.attributes.clone();
                if identity
                    == mainframe_env_ir::cics_executable_descriptor(
                        mainframe_env_ir::CicsPlanOperation::Getmain,
                    )
                    .identity()
                {
                    let mainframe_env_ir::Attribute::Bytes(bytes) = &attributes["cics_plan"] else {
                        panic!("compiled GETMAIN plan is missing");
                    };
                    let mut plan =
                        decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
                    plan.operation = mainframe_env_ir::CicsPlanOperation::Getmain64;
                    plan.operands[0].name = CicsOperandName::Flength64;
                    plan.operands.push(CicsNamedOperand {
                        name: CicsOperandName::Abi64,
                        value: CicsOperandValue::Literal(
                            b"mainframe-env.cics-amode64-nonle@1".to_vec(),
                        ),
                    });
                    plan.outputs[0].name = CicsOutputName::SetPointer64;
                    attributes.insert(
                        "cics_plan".into(),
                        mainframe_env_ir::Attribute::Bytes(
                            encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap(),
                        ),
                    );
                    identity = mainframe_env_ir::cics_executable_descriptor(
                        mainframe_env_ir::CicsPlanOperation::Getmain64,
                    )
                    .identity();
                    selected += 1;
                }
                builder
                    .add_operation(
                        new_block,
                        identity,
                        operation.operands.clone(),
                        operation.results.len(),
                        attributes,
                        operation.effects.clone(),
                        operation.storage.clone(),
                        operation.location.clone(),
                    )
                    .unwrap();
            }
        }
    }
    assert_eq!(selected, 1);
    encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
}

#[test]
fn compiled_selected_route_writes_only_a_checked_64_bit_address() {
    let binary = translated_getmain64();
    let mut invocation = invocation();
    invocation.bindings.insert(
        "cics.amode64.caller".into(),
        BoundedPayload::new(
            "mainframe-env.cics.amode64-caller@1",
            b"non-le-amode64".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    invocation.bindings.insert(
        "cics.amode64.taskdatakey".into(),
        BoundedPayload::new(
            "mainframe-env.cics.taskdatakey@1",
            b"USER".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    let mut machine =
        ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default()).unwrap();
    let effect = loop {
        match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("GETMAIN64 did not select a host call: {other:?}"),
        }
    };
    let HostRequest::Cics(request) = &effect.request else {
        panic!("unexpected host request");
    };
    assert_eq!(request.operation, CicsOperation::Getmain64);
    assert_eq!(
        request.arguments["ABI64"].bytes(),
        b"mainframe-env.cics-amode64-nonle@1"
    );
    assert_eq!(
        request.arguments["SET64.MAXLENGTH"].schema(),
        "mainframe-env.cics.decimal@1"
    );
    let response = mainframe_env_host_api::CicsResponse {
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
        payload: BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .unwrap(),
        outputs: BTreeMap::from([(
            "SET64".into(),
            BoundedPayload::new(
                "mainframe-env.cics.storage64-allocation@1",
                vec![0, 0, 0, 0, 0, 0, 0, 17],
                InvocationLimits::default(),
            )
            .unwrap(),
        )]),
        unit_of_work: None,
    };
    let result = mainframe_env_host_api::EffectResult {
        sequence: effect.sequence,
        outcome: Ok(HostResult::Cics(response)),
    };
    assert_eq!(
        machine.drive(
            MachineResume::HostResult(result),
            Quantum::new(1, 1024).unwrap()
        ),
        MachineDrive::Continue
    );
    let pointer = machine.variable("PTR-X").unwrap();
    assert_eq!(pointer.bytes().len(), 8);
    let address = u64::from_be_bytes(pointer.bytes().try_into().unwrap());
    assert_eq!(address >> 60, 0xA);
    assert_eq!(machine.read_storage64(address, 0, 17).unwrap().len(), 17);
    machine.write_storage64(address, 3, b"DATA").unwrap();
    assert_eq!(machine.read_storage64(address, 3, 4).unwrap(), b"DATA");
    let checkpoint = machine.checkpoint().unwrap();
    let mut restored =
        ReferenceMachine::from_binary(&binary, invocation, CodecLimits::default()).unwrap();
    restored.restore_checkpoint(&checkpoint).unwrap();
    assert_eq!(restored.variable("PTR-X").unwrap().bytes(), pointer.bytes());
    assert_eq!(restored.read_storage64(address, 0, 17).unwrap().len(), 17);
    assert_eq!(restored.read_storage64(address, 3, 4).unwrap(), b"DATA");
    for machine in [&mut machine, &mut restored] {
        loop {
            match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
                MachineDrive::Continue => continue,
                MachineDrive::Completed(_) => break,
                other => panic!("selected GETMAIN64 completion failed: {other:?}"),
            }
        }
        assert!(machine.read_storage64(address, 0, 1).is_err());
    }
}
