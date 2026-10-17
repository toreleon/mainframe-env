//! Actual compiler/machine/coordinator journal controls, not CICS product-journey credit.
use super::*;
use mainframe_env_compiler::CobolCompiler;
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService,
};
use mainframe_env_host_api::{CicsDisposition, CicsResponse};
use mainframe_env_interpreter::{CicsReadCopybackSelection, CicsReadCopybackSlotSelection};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use mainframe_env_store::StoreLimits;
use mainframe_env_store_api::{EffectRecoveryLease, ExecutionStore, IdempotencyStore};

struct ReadReply(CapabilityDescriptor);

impl HostProvider for ReadReply {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.0
    }

    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let HostRequest::Cics(request) = &effect.request else {
            panic!("expected actual compiled CICS effect");
        };
        assert!(matches!(
            request.operation,
            CicsOperation::Read | CicsOperation::Return
        ));
        let read = request.operation == CicsOperation::Read;
        let mut outputs = BTreeMap::new();
        if let Some(length) = request.arguments.get("LENGTH") {
            assert!(read);
            assert_eq!(length.schema(), "mainframe-env.cics.decimal@1");
            assert_eq!(length.bytes(), b"4");
            // Match the existing provider's actual-length receipt. This is
            // separate from the fixed synthetic DATA storage expectation.
            outputs.insert(
                "LENGTH".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.decimal@1",
                    b"DATA".len().to_string().into_bytes(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
        }
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Cics(CicsResponse {
                disposition: if read {
                    CicsDisposition::Complete
                } else {
                    CicsDisposition::Returned
                },
                condition: "NORMAL".into(),
                response: 0,
                response2: 0,
                applid: "TEST".into(),
                sysid: "TEST".into(),
                transaction: "TEST".into(),
                aid: 0,
                target: None,
                next_transaction: None,
                payload: BoundedPayload::new(
                    "mainframe-env.cics.into@1",
                    if read { b"DATA".to_vec() } else { Vec::new() },
                    InvocationLimits::default(),
                )
                .unwrap(),
                outputs,
                unit_of_work: None,
            })),
        }
    }
}

fn actual_journal(actor: &str) -> (Invocation, CicsReadCopybackArchive, Arc<MemoryStore>, u64) {
    actual_journal_source(actor, false)
}

fn actual_journal_source(
    actor: &str,
    length_of: bool,
) -> (Invocation, CicsReadCopybackArchive, Arc<MemoryStore>, u64) {
    let limits = SourceLimits::default();
    let path = LogicalPath::new("JOURNAL.cbl", limits.max_path_bytes).unwrap();
    let source: &[u8] = if length_of {
        b"IDENTIFICATION DIVISION. PROGRAM-ID. JOURNAL.
        DATA DIVISION. WORKING-STORAGE SECTION.
        01 KEY-X PIC X(2) VALUE 'AA'. 01 RECORD-X PIC X(4).
        PROCEDURE DIVISION.
        EXEC CICS READ FILE('FIXTURE') INTO(RECORD-X) RIDFLD(KEY-X)
          LENGTH(LENGTH OF RECORD-X) END-EXEC.
        EXEC CICS RETURN END-EXEC. STOP RUN."
    } else {
        b"IDENTIFICATION DIVISION. PROGRAM-ID. JOURNAL.
        DATA DIVISION. WORKING-STORAGE SECTION.
        01 KEY-X PIC X(2) VALUE 'AA'. 01 RECORD-X PIC X(4).
        PROCEDURE DIVISION.
        EXEC CICS READ FILE('FIXTURE') INTO(RECORD-X) RIDFLD(KEY-X) END-EXEC.
        EXEC CICS RETURN END-EXEC. STOP RUN."
    };
    let bundle = SourceBundle::new(
        &path,
        vec![
            SourceFile::input(
                path.as_str(),
                source.to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ],
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .unwrap();
    let compiled = CobolCompiler::default()
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        })
        .unwrap();
    let CompilerResult::Published { artifact, .. } = compiled else {
        panic!("fixture did not publish: {compiled:?}");
    };
    let capability = CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap();
    let mut invocation = invocation();
    invocation.execution_id = ExecutionId::new(actor, InvocationLimits::default()).unwrap();
    invocation.run_unit_id = RunUnitId::new(actor, InvocationLimits::default()).unwrap();
    invocation.artifact = ArtifactRef::new(
        artifact.content_id().to_reference(),
        InvocationLimits::default(),
    )
    .unwrap();
    invocation.principal = Principal::new(
        invocation.principal.id().clone(),
        BTreeSet::from([capability.clone()]),
        InvocationLimits::default(),
    )
    .unwrap();
    let module =
        mainframe_env_ir::decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
    let storage = module
        .storage()
        .iter()
        .find(|slot| slot.name == "record-x")
        .unwrap();
    let slot = CicsReadCopybackSlotSelection::new(
        CicsStorageSlot {
            storage: storage.id,
            qualified_layout_name: "RECORD-X".into(),
        },
        usize::try_from(storage.size).unwrap(),
    )
    .unwrap();
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .unwrap();
    machine
        .enable_read_copyback(
            CicsReadCopybackPlan::new(vec![CicsReadCopybackSelection::new(slot, None, None)])
                .unwrap(),
        )
        .unwrap();
    let provider: Arc<dyn HostProvider> = Arc::new(ReadReply(CapabilityDescriptor {
        capability,
        provider_id: "copyback-journal-test".into(),
        generation: "copyback-journal-test@1".into(),
        request_schema: "mainframe-env.cics.request@1".into(),
        result_schema: "mainframe-env.cics.result@1".into(),
        max_request_bytes: 4096,
        max_result_bytes: 4096,
        ready: true,
    }));
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![provider], InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let coordinator =
        ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
    let outcome = coordinator.execute(
        &mut machine,
        &invocation,
        mainframe_env_interpreter::ExecutionControl {
            now_tick: 10,
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(_)),
        "{outcome:?}"
    );
    let archive = machine.take_read_copyback().unwrap().ok().unwrap();
    assert_eq!(archive.reads().len(), 1);
    assert_eq!(archive.reads()[0].cells()[0].bytes(), b"DATA");
    let tick = store
        .get_execution(&invocation.execution_id)
        .unwrap()
        .unwrap()
        .terminal_tick
        .unwrap();
    (invocation, archive, store, tick)
}

#[test]
fn actual_read_and_return_join_their_genuine_completed_journal_records() {
    let (invocation, archive, store, tick) = actual_journal("original");
    for captured in [archive.reads()[0].effect(), archive.terminal()] {
        let effect = current_journal_effect(store.as_ref(), &invocation, captured, tick).unwrap();
        assert_eq!(effect.state, EffectState::Completed);
        recheck_journal_effect(store.as_ref(), &effect).unwrap();
    }
}

#[test]
fn compiled_length_of_read_and_return_join_their_genuine_completed_journal_records() {
    let (invocation, archive, store, tick) = actual_journal_source("length-of", true);
    for captured in [archive.reads()[0].effect(), archive.terminal()] {
        let effect = current_journal_effect(store.as_ref(), &invocation, captured, tick).unwrap();
        assert_eq!(effect.state, EffectState::Completed);
        recheck_journal_effect(store.as_ref(), &effect).unwrap();
    }
}

#[test]
fn actual_read_missing_and_foreign_journal_records_refuse() {
    let (invocation, archive, store, tick) = actual_journal("original");
    let captured = archive.reads()[0].effect();
    let empty = MemoryStore::new(StoreLimits::default());
    assert_eq!(
        current_journal_effect(&empty, &invocation, captured, tick),
        Err(CicsReadObservationProblem::Ownership)
    );
    let (_, _, foreign, _) = actual_journal("foreign");
    assert!(foreign.effect(captured.key().unwrap()).unwrap().is_some());
    assert_eq!(
        current_journal_effect(foreign.as_ref(), &invocation, captured, tick),
        Err(CicsReadObservationProblem::Ownership)
    );
    let effect = current_journal_effect(store.as_ref(), &invocation, captured, tick).unwrap();
    assert_eq!(
        recheck_journal_effect(&empty, &effect),
        Err(CicsReadObservationProblem::Ownership)
    );
    assert_eq!(
        recheck_journal_effect(foreign.as_ref(), &effect),
        Err(CicsReadObservationProblem::Ownership)
    );
}

#[test]
fn genuine_read_journal_field_changes_and_stale_observations_refuse() {
    let (invocation, archive, store, tick) = actual_journal("original");
    let captured = archive.reads()[0].effect();
    let actual = current_journal_effect(store.as_ref(), &invocation, captured, tick).unwrap();
    // Corrupt copies only as refusal inputs; no test constructs or writes an EffectRecord.
    for field in 0..23 {
        let mut changed = actual.clone();
        let limits = InvocationLimits::default();
        match field {
            0 => changed.execution_id = ExecutionId::new("foreign", limits).unwrap(),
            1 => changed.run_unit_id = RunUnitId::new("foreign", limits).unwrap(),
            2 => changed.sequence += 1,
            3 => changed.key = IdempotencyKey::new("foreign", limits).unwrap(),
            4 => changed.digest_format = EffectDigestFormat::LegacyDebug,
            5 => changed.request_digest[0] ^= 1,
            6 => changed.result_digest = None,
            7 => changed.state = EffectState::Intent,
            8 => changed.intent.owner = ExecutionId::new("foreign", limits).unwrap(),
            9 => changed.intent.attempt += 1,
            10 => changed.intent.audit_invocation_key = None,
            11 => changed.intent.audit_resource = None,
            12 => changed.intent.capability = None,
            13 => changed.intent.created_tick = 0,
            14 => changed.resolved_tick = None,
            15 => changed.resolved_tick = Some(tick + 1),
            16 => {
                changed.intent.recovery_lease = Some(EffectRecoveryLease {
                    owner: "foreign".into(),
                    attempt: 1,
                    epoch: 1,
                    expires_tick: tick + 1,
                })
            }
            17 => changed.result_digest.as_mut().unwrap()[0] ^= 1,
            18 => {
                changed.intent.capability =
                    Some(CapabilityId::new("host.dataset.read", limits).unwrap())
            }
            19 => changed.intent.audit_resource.as_mut().unwrap().value[0] ^= 1,
            20 => {
                changed.intent.audit_invocation_key =
                    Some(IdempotencyKey::new("foreign", limits).unwrap())
            }
            21 => changed.intent.created_tick = tick + 1,
            22 => changed.resolved_tick = Some(0),
            _ => unreachable!(),
        }
        assert!(
            !journal_effect_matches(&changed, &invocation, captured, tick),
            "field {field}"
        );
        assert_eq!(
            recheck_journal_effect(store.as_ref(), &changed),
            Err(CicsReadObservationProblem::Ownership),
            "field {field}"
        );
    }
    recheck_journal_effect(store.as_ref(), &actual).unwrap();
}
