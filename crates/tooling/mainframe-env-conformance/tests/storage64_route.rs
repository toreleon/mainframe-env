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
        BTreeMap::from([
            (
                "cics.amode64.caller".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.amode64-caller@1",
                    b"non-le-amode64".to_vec(),
                    limits,
                )
                .unwrap(),
            ),
            (
                "cics.amode64.taskdatakey".into(),
                BoundedPayload::new("mainframe-env.cics.taskdatakey@1", b"USER".to_vec(), limits)
                    .unwrap(),
            ),
        ]),
        limits,
    )
    .unwrap()
}

fn translated_storage64(free_data: Option<bool>) -> Vec<u8> {
    // The COBOL frontend supplies layout and control-flow scaffolding. This
    // test adapter replaces GETMAIN with a distinct checked AMODE(64) plan.
    let freemain = match free_data {
        Some(false) => "EXEC CICS FREEMAIN DATAPOINTER(PTR-X) END-EXEC.",
        Some(true) => "EXEC CICS FREEMAIN DATA(AREA-X) END-EXEC.",
        None => "",
    };
    let source = format!(
        "PROCESS LP(64)\nIDENTIFICATION DIVISION. PROGRAM-ID. STOR64. DATA DIVISION. WORKING-STORAGE SECTION. 01 PTR-X POINTER. 01 LEN-X PIC S9(9) COMP VALUE 17. 01 AREA-X PIC X(4). 01 FLAG-X PIC X. PROCEDURE DIVISION. EXEC CICS GETMAIN SET(PTR-X) FLENGTH(LEN-X) END-EXEC. MOVE 'Q' TO FLAG-X. {freemain} MOVE 'R' TO FLAG-X. STOP RUN."
    );
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
    let mut selected_get = 0;
    let mut selected_free = 0;
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
                    selected_get += 1;
                } else if identity
                    == mainframe_env_ir::cics_executable_descriptor(
                        mainframe_env_ir::CicsPlanOperation::Freemain,
                    )
                    .identity()
                {
                    let mainframe_env_ir::Attribute::Bytes(bytes) = &attributes["cics_plan"] else {
                        panic!("compiled FREEMAIN plan is missing");
                    };
                    let mut plan =
                        decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
                    plan.operation = mainframe_env_ir::CicsPlanOperation::Freemain64;
                    plan.operands[0].name = if free_data == Some(true) {
                        CicsOperandName::DataArea64
                    } else {
                        CicsOperandName::DataPointer64
                    };
                    plan.operands.push(CicsNamedOperand {
                        name: CicsOperandName::Abi64,
                        value: CicsOperandValue::Literal(
                            b"mainframe-env.cics-amode64-nonle@1".to_vec(),
                        ),
                    });
                    attributes.insert(
                        "cics_plan".into(),
                        mainframe_env_ir::Attribute::Bytes(
                            encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap(),
                        ),
                    );
                    identity = mainframe_env_ir::cics_executable_descriptor(
                        mainframe_env_ir::CicsPlanOperation::Freemain64,
                    )
                    .identity();
                    selected_free += 1;
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
    assert_eq!(selected_get, 1);
    assert_eq!(selected_free, usize::from(free_data.is_some()));
    encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap()
}

#[test]
fn compiled_selected_route_writes_only_a_checked_64_bit_address() {
    let binary = translated_storage64(None);
    let invocation = invocation();
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
    let mut forged =
        ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default()).unwrap();
    let forged_effect = loop {
        match forged.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
            MachineDrive::Continue => continue,
            MachineDrive::HostCall(effect) => break effect,
            other => panic!("forged GETMAIN64 did not select a host call: {other:?}"),
        }
    };
    let mut shared_response = response.clone();
    shared_response.outputs.insert(
        "SET64".into(),
        BoundedPayload::new(
            "mainframe-env.cics.storage64-allocation@1",
            vec![0, 0, 1, 0, 0, 0, 0, 17],
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    assert!(matches!(
        forged.drive(
            MachineResume::HostResult(mainframe_env_host_api::EffectResult {
                sequence: forged_effect.sequence,
                outcome: Ok(HostResult::Cics(shared_response)),
            }),
            Quantum::new(1, 1024).unwrap(),
        ),
        MachineDrive::Failed(_)
    ));
    assert!(forged.snapshot().storage64.allocations.is_empty());
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
    let mut shared_snapshot = machine.snapshot();
    shared_snapshot.storage64.allocations[0].attributes.shared = true;
    let mut refused =
        ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default()).unwrap();
    assert!(refused.restore(shared_snapshot).is_err());
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

#[test]
fn compiled_freemain64_pointer_and_bound_data_release_checkpointed_storage() {
    for data_form in [false, true] {
        let binary = translated_storage64(Some(data_form));
        let invocation = invocation();
        let mut machine =
            ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default())
                .unwrap();
        let allocate = loop {
            match machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
                MachineDrive::Continue => continue,
                MachineDrive::HostCall(effect) => break effect,
                other => panic!("GETMAIN64 was not selected: {other:?}"),
            }
        };
        let HostRequest::Cics(request) = &allocate.request else {
            panic!("unexpected GETMAIN64 request");
        };
        assert_eq!(request.operation, CicsOperation::Getmain64);
        let allocation = mainframe_env_host_api::CicsResponse {
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
        assert_eq!(
            machine.drive(
                MachineResume::HostResult(mainframe_env_host_api::EffectResult {
                    sequence: allocate.sequence,
                    outcome: Ok(HostResult::Cics(allocation)),
                }),
                Quantum::new(1, 1024).unwrap(),
            ),
            MachineDrive::Continue
        );
        let pointer = machine.variable("PTR-X").unwrap();
        let address = u64::from_be_bytes(pointer.bytes().try_into().unwrap());
        machine.write_storage64(address, 0, b"DATA").unwrap();
        if data_form {
            machine.bind_storage64_area("AREA-X", address).unwrap();
        }
        let checkpoint = machine.checkpoint().unwrap();
        assert_eq!(
            checkpoint.schema(),
            "mainframe-env.reference-machine-checkpoint@12"
        );
        let mut restored =
            ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default())
                .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.read_storage64(address, 0, 4).unwrap(), b"DATA");
        assert_eq!(
            restored.snapshot().storage64_area_bindings.len(),
            usize::from(data_form)
        );
        if data_form {
            let mut forged = machine.snapshot();
            forged
                .storage64_area_bindings
                .insert("AREA-X".into(), address + 1);
            let mut rejected =
                ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default())
                    .unwrap();
            assert!(rejected.restore(forged).is_err());
        }

        let release = loop {
            match restored.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
                MachineDrive::Continue => continue,
                MachineDrive::HostCall(effect) => break effect,
                other => panic!("FREEMAIN64 was not selected: {other:?}"),
            }
        };
        let HostRequest::Cics(request) = &release.request else {
            panic!("unexpected FREEMAIN64 request");
        };
        assert_eq!(request.operation, CicsOperation::Freemain64);
        let operand = if data_form { "DATA" } else { "DATAPOINTER" };
        let release_pointer = request.arguments[operand].clone();
        assert_eq!(
            release_pointer.schema(),
            "mainframe-env.cics.allocated-pointer64@1"
        );
        assert_eq!(release_pointer.bytes(), pointer.bytes());
        if data_form {
            assert_ne!(
                restored.variable("AREA-X").unwrap().bytes(),
                pointer.bytes()
            );
        }
        let freed = mainframe_env_host_api::CicsResponse {
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
            outputs: BTreeMap::from([("FREEMAIN64.POINTER".into(), release_pointer)]),
            unit_of_work: None,
        };
        if !data_form {
            let mut forged =
                ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default())
                    .unwrap();
            forged.restore_checkpoint(&checkpoint).unwrap();
            let forged_effect = loop {
                match forged.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()) {
                    MachineDrive::Continue => continue,
                    MachineDrive::HostCall(effect) => break effect,
                    other => panic!("forged release did not reach host: {other:?}"),
                }
            };
            let mut wrong = freed.clone();
            wrong.outputs.insert(
                "FREEMAIN64.POINTER".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.allocated-pointer64@1",
                    (address + 1).to_be_bytes().to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            assert!(matches!(
                forged.drive(
                    MachineResume::HostResult(mainframe_env_host_api::EffectResult {
                        sequence: forged_effect.sequence,
                        outcome: Ok(HostResult::Cics(wrong)),
                    }),
                    Quantum::new(1, 1024).unwrap(),
                ),
                MachineDrive::Failed(_)
            ));
            assert_eq!(forged.read_storage64(address, 0, 4).unwrap(), b"DATA");
        }
        assert_eq!(
            restored.drive(
                MachineResume::HostResult(mainframe_env_host_api::EffectResult {
                    sequence: release.sequence,
                    outcome: Ok(HostResult::Cics(freed)),
                }),
                Quantum::new(1, 1024).unwrap(),
            ),
            MachineDrive::Continue
        );
        assert!(restored.read_storage64(address, 0, 1).is_err());
        assert!(restored.snapshot().storage64.allocations.is_empty());
        assert!(restored.snapshot().storage64_area_bindings.is_empty());
        assert!(restored.bind_storage64_area("AREA-X", address).is_err());
        let released_checkpoint = restored.checkpoint().unwrap();
        let mut reopened =
            ReferenceMachine::from_binary(&binary, invocation, CodecLimits::default()).unwrap();
        reopened.restore_checkpoint(&released_checkpoint).unwrap();
        assert!(reopened.read_storage64(address, 0, 1).is_err());
    }
}
