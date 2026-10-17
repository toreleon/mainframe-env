//! Compiled local DTO/storage controls, not first-party or raw DL/I ABI acceptance.
use super::*;
use mainframe_env_execution_api::{AuditDecision, ExecutionOutcome};
use mainframe_env_host_api::{
    EffectResult, HostLimits, HostRequest, HostResult, ImsResult, ImsSegment,
};
use mainframe_env_ims::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsPcbMetadata, ImsPsbMetadata,
    ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_store_api::{
    AuditSink, EffectState, ExecutionState, ExecutionStore, IdempotencyStore,
};

fn quantum() -> Quantum {
    Quantum {
        max_steps: 100,
        max_allocated_bytes: 1_048_576,
    }
}

fn machine(source: &str, run: &str) -> (ReferenceMachine, Invocation) {
    let definition = compile_free_batch_definition("IMSBRDG", source).unwrap();
    let mut invocation = ims_invocation(run, true, None).unwrap();
    invocation.artifact = definition.artifact;
    let machine = ReferenceMachine::from_binary(
        &definition.payload,
        invocation.clone(),
        mainframe_env_ir::CodecLimits::default(),
    )
    .unwrap();
    (machine, invocation)
}

fn source(storage: &str, body: &str) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. IMSBRDG.\n\
         DATA DIVISION. WORKING-STORAGE SECTION.\n\
         01 ROOT-X PIC X(100) VALUE ALL 'Z'.\n\
         {storage}\n\
         PROCEDURE DIVISION. {body} STOP RUN."
    )
}

fn pending(storage: &str) -> (ReferenceMachine, EffectRequest) {
    let (mut machine, _) = machine(
        &source(
            storage,
            "EXEC DLI GU USING PCB(1) SEGMENT(PAUTSUM0) INTO(ROOT-X) END-EXEC. \
             DISPLAY ROOT-X. DISPLAY DIBSTAT.",
        ),
        "ims-copyback-reply",
    );
    let MachineDrive::HostCall(effect) = machine.drive(MachineResume::Start, quantum()) else {
        panic!("actual compiled EXEC DLI offer required")
    };
    effect.validate(HostLimits::default()).unwrap();
    let HostRequest::Ims(request) = &effect.request else {
        panic!("existing typed IMS route required")
    };
    assert_eq!(request.segments, ["PAUTSUM0"]);
    assert_eq!(request.max_segments, 1);
    (machine, effect)
}

fn reply(status: &str, name: &str, data: Vec<u8>) -> ImsResult {
    ImsResult {
        status: status.into(),
        segments: vec![ImsSegment {
            name: name.into(),
            parent_key: None,
            data,
        }],
        checkpoint_id: None,
        affected_segments: 0,
        system: None,
    }
}

fn resume(
    machine: &mut ReferenceMachine,
    sequence: u64,
    result: ImsResult,
) -> MachineDrive<EffectRequest> {
    machine.drive(
        MachineResume::HostResult(EffectResult {
            sequence,
            outcome: Ok(HostResult::Ims(result)),
        }),
        quantum(),
    )
}

#[test]
fn compiled_dli_exact_area_and_status_only_preserve_complete_output() {
    for has_segment in [true, false] {
        let (mut machine, effect) = pending("01 DIBSTAT PIC X(2) VALUE '!!'.");
        let mut result = reply("GE", "PAUTSUM0", vec![b'R'; 100]);
        if !has_segment {
            result.segments.clear();
        }
        let MachineDrive::Completed(completion) = resume(&mut machine, effect.sequence, result)
        else {
            panic!("exact-width or status-only reply must complete")
        };
        let expected = [
            vec![if has_segment { b'R' } else { b'Z' }; 100],
            b"\nGE\n".to_vec(),
        ]
        .concat();
        assert_eq!(completion.return_code, 0);
        assert_eq!(completion.output.bytes(), expected);
    }
}

#[test]
fn compiled_dli_short_overflow_multiple_and_foreign_results_preserve_all_storage() {
    let mut cases = vec![
        reply("XY", "PAUTSUM0", vec![]),
        reply("XY", "PAUTSUM0", vec![b'R'; 99]),
        reply("XY", "PAUTSUM0", vec![b'R'; 101]),
        reply("XY", "PAUTDTL1", vec![b'R'; 100]),
        reply("X", "PAUTSUM0", vec![b'R'; 100]),
        reply("XYZ", "PAUTSUM0", vec![b'R'; 100]),
    ];
    let mut multiple = reply("XY", "PAUTSUM0", vec![b'R'; 100]);
    multiple.segments.push(multiple.segments[0].clone());
    cases.push(multiple);
    for result in cases {
        let (mut machine, effect) = pending("01 DIBSTAT PIC X(2) VALUE '!!'.");
        let before = machine.snapshot();
        assert!(matches!(
            resume(&mut machine, effect.sequence, result),
            MachineDrive::Failed(_)
        ));
        let after = machine.snapshot();
        assert_eq!(after.base_storage, before.base_storage);
        assert_eq!(after.implicit_values, before.implicit_values);
        assert_eq!(after.output, before.output);
    }
}

#[test]
fn compiled_dli_structured_groups_do_not_create_positional_qualifiers() {
    for (command, expected) in [
        (
            "EXEC DLI GNP USING PCB(PCB-N) SEGMENT(PAUTSUM0) INTO(ROOT-X) END-EXEC.",
            vec![],
        ),
        (
            "EXEC DLI GU USING PCB-N ACCT-X END-EXEC.",
            vec![ImsQualifier {
                segment: "ROOT".into(),
                field: "ACCT-X".into(),
                value: b"000001".to_vec(),
            }],
        ),
    ] {
        let (mut machine, _) = machine(
            &source(
                "01 PCB-N PIC S9(4) COMP VALUE 1. 01 ACCT-X PIC X(6) VALUE '000001'.",
                command,
            ),
            "ims-copyback-group-separation",
        );
        let MachineDrive::HostCall(effect) = machine.drive(MachineResume::Start, quantum()) else {
            panic!("actual source must emit its own existing request family")
        };
        let HostRequest::Ims(request) = effect.request else {
            panic!("existing typed IMS request required")
        };
        assert_eq!(request.qualifiers, expected);
    }
}

#[test]
fn compiled_dli_overlapping_declared_status_refuses_before_offer() {
    let (mut machine, _) = machine(
        &source(
            "01 DIBSTAT REDEFINES ROOT-X PIC X(2).",
            "EXEC DLI GU USING PCB(1) SEGMENT(PAUTSUM0) INTO(ROOT-X) END-EXEC.",
        ),
        "ims-copyback-overlap",
    );
    let before = machine.snapshot();
    assert!(matches!(
        machine.drive(MachineResume::Start, quantum()),
        MachineDrive::Failed(_)
    ));
    let after = machine.snapshot();
    // Entry initialization is allowed before the rejected call; no effect is offered.
    assert_eq!(after.effect_sequence, before.effect_sequence);
    assert!(after.output.is_empty());
}

fn catalog() -> ImsMetadataCatalog {
    fn segment(
        name: &str,
        parent: Option<&str>,
        key: &str,
        width: usize,
        key_width: usize,
    ) -> ImsSegmentMetadata {
        ImsSegmentMetadata {
            name: name.into(),
            parent: parent.map(str::to_owned),
            min_length: width,
            max_length: width,
            fields: vec![ImsFieldMetadata {
                name: Some(key.into()),
                offset: 0,
                length: key_width,
                sequence: true,
                unique: true,
            }],
        }
    }
    // Exact pinned root/child/key widths; records below are synthetic local controls.
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            gsam_format: None,
            name: "BRGDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Hidam,
            segments: vec![
                segment("PAUTSUM0", None, "ACCNTID", 100, 6),
                segment("PAUTDTL1", Some("PAUTSUM0"), "PAUT9CTS", 200, 8),
            ],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "BRGPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "BRGPCB".into(),
                database: "BRGDB".into(),
                database_version: Some(1),
                secondary_index: None,
                processing_options: "AP".into(),
                sensitive_segments: vec![
                    ImsSensitiveSegmentMetadata {
                        name: "PAUTSUM0".into(),
                        parent: None,
                        processing_options: None,
                    },
                    ImsSensitiveSegmentMetadata {
                        name: "PAUTDTL1".into(),
                        parent: Some("PAUTSUM0".into()),
                        processing_options: None,
                    },
                ],
            })],
        }],
    }
}

fn provider_source() -> &'static str {
    "IDENTIFICATION DIVISION. PROGRAM-ID. IMSBRDG.\n\
     DATA DIVISION. WORKING-STORAGE SECTION.\n\
     01 ROOT-IN. 05 PA-ACCT-ID PIC S9(11) COMP-3 VALUE 1.\n\
     05 ROOT-FILL PIC X(94) VALUE ALL 'R'.\n\
     01 CHILD-IN. 05 CHILD-KEY PIC X(8) VALUE 'CHILD001'.\n\
     05 CHILD-FILL PIC X(192) VALUE ALL 'C'.\n\
     01 ROOT-X PIC X(100) VALUE ALL 'Z'.\n\
     01 CHILD-X PIC X(200) VALUE ALL 'Y'.\n\
     PROCEDURE DIVISION.\n\
     EXEC DLI SCHD PSB('BRGPSB') USING PCB(1) END-EXEC.\n\
     EXEC DLI ISRT USING PCB(1) SEGMENT(PAUTSUM0) FROM(ROOT-IN) END-EXEC.\n\
     EXEC DLI ISRT USING PCB(1) SEGMENT(PAUTSUM0) WHERE(ACCNTID = PA-ACCT-ID)\n\
       SEGMENT(PAUTDTL1) FROM(CHILD-IN) END-EXEC.\n\
     EXEC DLI GU USING PCB(1) SEGMENT(PAUTSUM0) INTO(ROOT-X)\n\
       WHERE(ACCNTID = PA-ACCT-ID) END-EXEC.\n\
     DISPLAY ROOT-X. DISPLAY DIBSTAT.\n\
     EXEC DLI GNP USING PCB(1) SEGMENT(PAUTDTL1) INTO(CHILD-X) END-EXEC.\n\
     DISPLAY CHILD-X. DISPLAY DIBSTAT.\n\
     EXEC DLI GNP USING PCB(1) SEGMENT(PAUTDTL1) INTO(CHILD-X) END-EXEC.\n\
     DISPLAY CHILD-X. DISPLAY DIBSTAT.\n\
     EXEC DLI TERM END-EXEC. DISPLAY DIBSTAT. STOP RUN."
}

fn expected_provider_output() -> Vec<u8> {
    let root = [vec![0, 0, 0, 0, 0, 0x1c], vec![b'R'; 94]].concat();
    let child = [b"CHILD001".to_vec(), vec![b'C'; 192]].concat();
    [
        root,
        b"\n  \n".to_vec(),
        child.clone(),
        b"\n  \n".to_vec(),
        child,
        b"\nGE\n  \n".to_vec(),
    ]
    .concat()
}

#[test]
fn compiled_dli_actual_provider_root_child_storage_and_journal_complete() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    let host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                ims_providers(service, InvocationLimits::default()),
                InvocationLimits::default(),
            )
            .unwrap(),
        ),
        HostLimits::default(),
    ));
    let (mut machine, invocation) = machine(provider_source(), "ims-copyback-provider");
    let outcome = ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default())
        .execute(
            &mut machine,
            &invocation,
            ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            },
        );
    let ExecutionOutcome::Completed(completion) = outcome else {
        panic!("actual compiled/provider path must complete: {outcome:?}")
    };
    assert_eq!(completion.return_code, 0);
    assert_eq!(completion.output.bytes(), expected_provider_output());
    assert_eq!(machine.effect_sequence(), 7);
    let execution = store
        .get_execution(&invocation.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(execution.state, ExecutionState::Completed);
    assert_eq!(execution.artifact, invocation.artifact);
    let audits = store.audit_records(&invocation.execution_id, 0, 8).unwrap();
    assert_eq!(audits.len(), 7);
    assert!(
        audits
            .iter()
            .all(|audit| audit.decision == AuditDecision::Success)
    );
    for sequence in 1..=7 {
        let key = IdempotencyKey::new(
            format!("{}:{sequence}", invocation.idempotency_key.as_str()),
            InvocationLimits::default(),
        )
        .unwrap();
        let effect = store.effect(&key).unwrap().unwrap();
        assert_eq!(effect.sequence, sequence);
        assert_eq!(effect.run_unit_id, invocation.run_unit_id);
        assert_eq!(effect.execution_id, invocation.execution_id);
        assert_eq!(effect.state, EffectState::Completed);
        assert!(effect.result_digest.is_some());
    }
}

#[test]
fn compiled_dli_actual_provider_reopen_replay_and_initial_snapshot_reoffer() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let mut service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    let (mut original_machine, invocation) = machine(provider_source(), "ims-copyback-replay");
    let initial = original_machine.snapshot();
    let (mut reconstructed, _) = machine(provider_source(), "ims-copyback-replay");
    reconstructed.restore(initial).unwrap();
    let first = original_machine.drive(MachineResume::Start, quantum());
    let restored = reconstructed.drive(MachineResume::Start, quantum());
    assert_eq!(first, restored);
    let mut drive = first;
    let mut count = 0;
    loop {
        match drive {
            MachineDrive::HostCall(effect) => {
                effect.validate(HostLimits::default()).unwrap();
                let HostRequest::Ims(request) = &effect.request else {
                    panic!("compiled IMS request required")
                };
                // Same actual store, reopened provider, exact admitted original effect operands.
                service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
                let result = service.execute(&invocation, request).unwrap();
                assert_eq!(service.execute(&invocation, request).unwrap(), result);
                count += 1;
                drive = resume(&mut original_machine, effect.sequence, result);
            }
            MachineDrive::Continue => {
                drive = original_machine.drive(MachineResume::Start, quantum())
            }
            MachineDrive::Completed(completion) => {
                assert_eq!(count, 7);
                assert_eq!(completion.output.bytes(), expected_provider_output());
                break;
            }
            other => panic!("actual provider replay path refused: {other:?}"),
        }
    }
}
