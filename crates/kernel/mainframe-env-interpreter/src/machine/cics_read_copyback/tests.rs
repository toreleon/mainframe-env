//! Synthetic typed runtime controls, not corpus or licensed execution credit.
use super::*;
use mainframe_env_execution_api::{Principal, PrincipalId};
use mainframe_env_host_api::CicsResponse;
use mainframe_env_ir::{IrLimits, ModuleBuilder, encode_binary};

const RESPONSE: &str = "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-RESP-CD";
const RESPONSE2: &str = "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-REAS-CD";
const INTO: [(&str, usize); 3] = [
    ("CARD-XREF-RECORD", 50),
    ("ACCOUNT-RECORD", 300),
    ("CUSTOMER-RECORD", 500),
];

fn plan(machine: &ReferenceMachine) -> CicsReadCopybackPlan {
    let selected = |name: &str, length| {
        let typed_cics::CicsTarget::Resolved(descriptor) = target(machine, name) else {
            unreachable!();
        };
        CicsReadCopybackSlotSelection::new(descriptor, length).unwrap()
    };
    CicsReadCopybackPlan::new(
        INTO.into_iter()
            .map(|(name, length)| {
                CicsReadCopybackSelection::new(
                    selected(name, length),
                    Some(selected(RESPONSE, 4)),
                    Some(selected(RESPONSE2, 4)),
                )
            })
            .collect(),
    )
    .unwrap()
}

fn machine(enabled: bool) -> ReferenceMachine {
    let mut invocation = super::super::tests::invocation();
    invocation.selector = Selector::new("program:COACTVWC", InvocationLimits::default()).unwrap();
    invocation.principal = Principal::new(
        PrincipalId::new("WEBUSER", InvocationLimits::default()).unwrap(),
        BTreeSet::new(),
        InvocationLimits::default(),
    )
    .unwrap();
    invocation.bindings.insert(
        "cics.commarea".into(),
        BoundedPayload::new(
            "mainframe-env.cics.commarea@1",
            vec![0; 2000],
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    invocation.bindings.insert(
        "cics.aid".into(),
        BoundedPayload::new(
            "mainframe-env.cics.aid@1",
            vec![0x7d],
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    invocation.bindings.insert(
        "cics.transaction".into(),
        BoundedPayload::new(
            "mainframe-env.cics.transaction@1",
            b"CAVW".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    let mut builder = ModuleBuilder::new(IrLimits::default());
    for (name, length) in INTO.into_iter().chain([(RESPONSE, 4), (RESPONSE2, 4)]) {
        builder
            .add_storage(name, u64::try_from(length).unwrap(), None)
            .unwrap();
    }
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "halt", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
    let binary = encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap();
    let mut machine =
        ReferenceMachine::from_binary(&binary, invocation, CodecLimits::default()).unwrap();
    // This control exercises the actual typed reply/copyback path with resolved
    // runtime fixtures. The first-party compiler/artifact owner is a separate gate.
    for (name, length) in INTO.into_iter().chain([(RESPONSE, 4), (RESPONSE2, 4)]) {
        let binary = name == RESPONSE || name == RESPONSE2;
        machine.layouts.insert(
            name.into(),
            LayoutMetadata {
                name: name.into(),
                simple_name: name.rsplit('.').next().unwrap().into(),
                category: if binary {
                    LayoutCategory::Binary
                } else {
                    LayoutCategory::Group
                },
                picture: if binary {
                    "S9(9) COMP".into()
                } else {
                    String::new()
                },
                digits: if binary { 9 } else { 0 },
                scale: 0,
                native_binary: false,
                signed: binary,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                linkage: false,
                offset: 0,
                length,
                element_length: length,
                occurs: 1,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                dynamic: false,
                dynamic_limit: 0,
                parent: None,
                alias_of: None,
                occurs_clause: false,
                condition_values: Vec::new(),
                object_class: None,
            },
        );
    }
    if enabled {
        machine.enable_read_copyback(plan(&machine)).unwrap();
    }
    machine
}

fn target(machine: &ReferenceMachine, name: &str) -> typed_cics::CicsTarget {
    let id = machine
        .storage_names_by_id
        .iter()
        .find(|(_, value)| value.as_str() == name)
        .unwrap()
        .0;
    typed_cics::CicsTarget::Resolved(CicsStorageSlot {
        storage: *id,
        qualified_layout_name: name.into(),
    })
}

fn offer(machine: &mut ReferenceMachine, ordinal: usize) -> EffectRequest {
    offer_with_bindings(machine, ordinal, BTreeMap::new(), BTreeMap::new())
}

fn offer_with_bindings(
    machine: &mut ReferenceMachine,
    ordinal: usize,
    arguments: BTreeMap<String, BoundedPayload>,
    outputs: BTreeMap<String, typed_cics::CicsTarget>,
) -> EffectRequest {
    let into = target(machine, INTO[ordinal].0);
    let response = target(machine, RESPONSE);
    let response2 = target(machine, RESPONSE2);
    let Step::Effect(effect) = machine
        .effect(
            HostRequest::Cics(CicsRequest {
                operation: CicsOperation::Read,
                arguments,
                mutation: None,
                condition_policy: CicsConditionPolicy::Respond {
                    response_field: RESPONSE.into(),
                    response2_field: Some(RESPONSE2.into()),
                },
            }),
            PendingKind::Cics {
                operation: CicsOperation::Read,
                storage64_intent: None,
                argument_summary: String::new(),
                container: None,
                into: Some(into),
                outputs,
                response: Some(response),
                response2: Some(response2),
                address_set: None,
                no_handle: false,
            },
        )
        .unwrap()
    else {
        panic!("expected read");
    };
    *effect
}

fn reply(sequence: u64, bytes: Vec<u8>, disposition: CicsDisposition) -> EffectResult {
    EffectResult {
        sequence,
        outcome: Ok(HostResult::Cics(CicsResponse {
            disposition,
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            applid: "TEST".into(),
            sysid: "TEST".into(),
            transaction: "CAVW".into(),
            aid: 0x7d,
            target: None,
            next_transaction: None,
            payload: BoundedPayload::new(
                "mainframe-env.cics.into@1",
                bytes,
                InvocationLimits::default(),
            )
            .unwrap(),
            outputs: BTreeMap::new(),
            unit_of_work: None,
        })),
    }
}

fn apply(machine: &mut ReferenceMachine, ordinal: usize) {
    let effect = offer(machine, ordinal);
    let bytes = vec![b'A' + u8::try_from(ordinal).unwrap(); INTO[ordinal].1];
    machine
        .resume_host(reply(effect.sequence, bytes, CicsDisposition::Complete))
        .unwrap();
}

fn finish(machine: &mut ReferenceMachine) {
    let mutation = machine.mutation().unwrap();
    let Step::Effect(effect) = machine
        .effect(
            HostRequest::Cics(CicsRequest {
                operation: CicsOperation::Return,
                arguments: BTreeMap::new(),
                mutation: Some(mutation),
                condition_policy: CicsConditionPolicy::Default,
            }),
            PendingKind::Cics {
                operation: CicsOperation::Return,
                storage64_intent: None,
                argument_summary: String::new(),
                container: None,
                into: None,
                outputs: BTreeMap::new(),
                response: None,
                response2: None,
                address_set: None,
                no_handle: false,
            },
        )
        .unwrap()
    else {
        panic!("expected return");
    };
    machine
        .resume_host(reply(
            effect.sequence,
            Vec::new(),
            CicsDisposition::Returned,
        ))
        .unwrap();
}

#[test]
fn postapplication_cells_survive_later_machine_overwrites() {
    let mut machine = machine(true);
    for (ordinal, &(name, length)) in INTO.iter().enumerate() {
        apply(&mut machine, ordinal);
        machine.write_raw(name, &vec![b'Z'; length]).unwrap();
        machine.write_raw(RESPONSE, &[9; 4]).unwrap();
        machine
            .implicit
            .insert("EIBFN".into(), CobolValue::Bytes(vec![9, 9]));
    }
    finish(&mut machine);
    let archive = machine.take_read_copyback().unwrap().ok().unwrap();
    for (ordinal, read) in archive.reads().iter().enumerate() {
        assert_eq!(
            read.cells()[0].bytes(),
            vec![b'A' + ordinal as u8; INTO[ordinal].1]
        );
        assert_eq!(read.cells()[1].bytes(), [0; 4]);
        assert_eq!(read.cells()[2].bytes(), [0; 4]);
        assert!(
            read.eib()
                .contains(&("EIBFN", CicsReadCopybackValue::Bytes(vec![6, 2])))
        );
    }
    assert_eq!(
        machine.variable("CARD-XREF-RECORD").unwrap().bytes(),
        vec![b'Z'; 50]
    );
}

#[test]
fn wrong_sequence_and_unknown_outcome_keep_original_refusals() {
    for unknown in [false, true] {
        let mut observed = machine(true);
        let mut plain = machine(false);
        let effect = offer(&mut observed, 0);
        offer(&mut plain, 0);
        let result = if unknown {
            EffectResult {
                sequence: effect.sequence,
                outcome: Err(HostProblem::UnknownOutcome),
            }
        } else {
            reply(
                effect.sequence + 1,
                vec![b'A'; 50],
                CicsDisposition::Complete,
            )
        };
        assert_eq!(
            observed.resume_host(result.clone()),
            plain.resume_host(result)
        );
        assert!(matches!(observed.take_read_copyback(), Some(Err(_))));
    }
}

#[test]
fn partial_projection_failure_never_publishes_a_success() {
    let mut machine = machine(true);
    let effect = offer(&mut machine, 0);
    machine.implicit.insert(
        "EIBFN".into(),
        CobolValue::Decimal(Decimal {
            coefficient: 0,
            scale: 0,
        }),
    );
    assert!(
        machine
            .resume_host(reply(
                effect.sequence,
                vec![b'A'; 50],
                CicsDisposition::Complete
            ))
            .is_err()
    );
    assert_eq!(
        machine.variable("CARD-XREF-RECORD").unwrap().bytes(),
        vec![b'A'; 50]
    );
    assert!(matches!(
        machine.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Application))
    ));
}

#[test]
fn armed_capture_preserves_original_runtime_state_effects_and_budgets() {
    let mut observed = machine(true);
    let mut plain = machine(false);
    let limits = plain.invocation.limits;
    for (ordinal, &(_, length)) in INTO.iter().enumerate() {
        let actual = offer(&mut observed, ordinal);
        let expected = offer(&mut plain, ordinal);
        assert_eq!(actual, expected);
        let result = reply(
            actual.sequence,
            vec![b'A' + ordinal as u8; length],
            CicsDisposition::Complete,
        );
        assert_eq!(
            observed.resume_host(result.clone()),
            plain.resume_host(result)
        );
        assert_eq!(observed.snapshot(), plain.snapshot());
    }
    finish(&mut observed);
    finish(&mut plain);
    assert_eq!(observed.snapshot(), plain.snapshot());
    assert_eq!(observed.invocation.limits, limits);
    assert!(plain.take_read_copyback().is_none());
}

fn length_payload(schema: &str, bytes: &[u8]) -> BoundedPayload {
    BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap()
}

fn full_length_argument(ordinal: usize) -> BTreeMap<String, BoundedPayload> {
    BTreeMap::from([(
        "LENGTH".into(),
        length_payload(
            "mainframe-env.cics.decimal@1",
            INTO[ordinal].1.to_string().as_bytes(),
        ),
    )])
}

fn length_reply(sequence: u64, outputs: BTreeMap<String, BoundedPayload>) -> EffectResult {
    let mut result = reply(sequence, vec![b'A'; 50], CicsDisposition::Complete);
    let Ok(HostResult::Cics(response)) = &mut result.outcome else {
        unreachable!();
    };
    response.outputs = outputs;
    result
}

#[test]
fn full_slot_length_receipts_preserve_original_execution_and_complete_capture() {
    let mut observed = machine(true);
    let mut plain = machine(false);
    for (ordinal, &(_, width)) in INTO.iter().enumerate() {
        let arguments = full_length_argument(ordinal);
        let actual =
            offer_with_bindings(&mut observed, ordinal, arguments.clone(), BTreeMap::new());
        let expected = offer_with_bindings(&mut plain, ordinal, arguments.clone(), BTreeMap::new());
        assert_eq!(actual, expected);
        let mut result = reply(
            actual.sequence,
            vec![b'A' + ordinal as u8; width],
            CicsDisposition::Complete,
        );
        let Ok(HostResult::Cics(response)) = &mut result.outcome else {
            unreachable!();
        };
        response.outputs = arguments;
        assert_eq!(
            observed.resume_host(result.clone()),
            plain.resume_host(result)
        );
        assert_eq!(observed.snapshot(), plain.snapshot());
    }
    finish(&mut observed);
    finish(&mut plain);
    assert_eq!(observed.snapshot(), plain.snapshot());
    let archive = observed.take_read_copyback().unwrap().unwrap();
    assert_eq!(archive.reads().len(), 3);
    for (ordinal, read) in archive.reads().iter().enumerate() {
        assert!(independent_synthetic_storage_matches(read, ordinal));
    }
    assert!(plain.take_read_copyback().is_none());
}

#[test]
fn requested_length_refuses_missing_wrong_noncanonical_and_extra_receipts() {
    let canonical = || length_payload("mainframe-env.cics.decimal@1", b"50");
    let mut cases = vec![BTreeMap::new()];
    for bytes in [
        b"49".as_slice(),
        b"51",
        b"0",
        b"-50",
        b"+50",
        b"050",
        b" 50",
    ] {
        cases.push(BTreeMap::from([(
            "LENGTH".into(),
            length_payload("mainframe-env.cics.decimal@1", bytes),
        )]));
    }
    cases.push(BTreeMap::from([(
        "LENGTH".into(),
        length_payload("mainframe-env.cics.literal@1", b"50"),
    )]));
    cases.push(BTreeMap::from([("OTHER".into(), canonical())]));
    for name in ["OTHER", "TASK.PRIORITY", "BMS.FIELD"] {
        cases.push(BTreeMap::from([
            ("LENGTH".into(), canonical()),
            (name.into(), canonical()),
        ]));
    }
    for outputs in cases {
        let mut observed = machine(true);
        let effect =
            offer_with_bindings(&mut observed, 0, full_length_argument(0), BTreeMap::new());
        let result = length_reply(effect.sequence, outputs);
        let _ = observed.resume_host(result);
        assert!(matches!(
            observed.take_read_copyback(),
            Some(Err(CicsReadCopybackProblem::Application))
        ));
    }
    let mut observed = machine(true);
    let effect = offer(&mut observed, 0);
    observed
        .resume_host(length_reply(
            effect.sequence,
            BTreeMap::from([("LENGTH".into(), canonical())]),
        ))
        .unwrap();
    assert!(matches!(
        observed.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Application))
    ));
}

#[test]
fn supplied_length_must_be_canonical_full_slot_input_without_writable_outputs() {
    for bytes in [
        b"49".as_slice(),
        b"51",
        b"0",
        b"-50",
        b"+50",
        b"050",
        b" 50",
    ] {
        let mut observed = machine(true);
        let arguments = BTreeMap::from([(
            "LENGTH".into(),
            length_payload("mainframe-env.cics.decimal@1", bytes),
        )]);
        offer_with_bindings(&mut observed, 0, arguments, BTreeMap::new());
        assert!(matches!(
            observed.take_read_copyback(),
            Some(Err(CicsReadCopybackProblem::Layout))
        ));
    }
    let mut observed = machine(true);
    let arguments = BTreeMap::from([(
        "LENGTH".into(),
        length_payload("mainframe-env.cics.literal@1", b"50"),
    )]);
    offer_with_bindings(&mut observed, 0, arguments, BTreeMap::new());
    assert!(matches!(
        observed.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Layout))
    ));
    let mut observed = machine(true);
    let writable = BTreeMap::from([("LENGTH".into(), target(&observed, RESPONSE))]);
    offer_with_bindings(&mut observed, 0, full_length_argument(0), writable);
    assert!(matches!(
        observed.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Layout))
    ));
}

#[test]
fn length_receipt_never_qualifies_truncation_or_non_normal_completion() {
    for non_normal in [false, true] {
        let mut observed = machine(true);
        let effect =
            offer_with_bindings(&mut observed, 0, full_length_argument(0), BTreeMap::new());
        let mut result = length_reply(effect.sequence, full_length_argument(0));
        let Ok(HostResult::Cics(response)) = &mut result.outcome else {
            unreachable!();
        };
        if non_normal {
            response.condition = "LENGERR".into();
            response.response = 22;
        } else {
            response.payload = BoundedPayload::new(
                "mainframe-env.cics.into@1",
                vec![b'A'; 49],
                InvocationLimits::default(),
            )
            .unwrap();
        }
        let _ = observed.resume_host(result);
        assert!(matches!(
            observed.take_read_copyback(),
            Some(Err(CicsReadCopybackProblem::Application))
        ));
    }
}

fn independent_synthetic_storage_matches(read: &CicsReadCopybackRecord, ordinal: usize) -> bool {
    let expected_eib = vec![
        (
            "EIBRESP",
            CicsReadCopybackValue::Decimal {
                coefficient: 0,
                scale: 0,
            },
        ),
        (
            "EIBRESP2",
            CicsReadCopybackValue::Decimal {
                coefficient: 0,
                scale: 0,
            },
        ),
        ("EIBFN", CicsReadCopybackValue::Bytes(vec![6, 2])),
        ("EIBAID", CicsReadCopybackValue::Bytes(vec![0x7d])),
        (
            "EIBCALEN",
            CicsReadCopybackValue::Decimal {
                coefficient: 2000,
                scale: 0,
            },
        ),
        ("EIBTRNID", CicsReadCopybackValue::Bytes(b"CAVW".to_vec())),
    ];
    read.cells.len() == 3
        && read.cells[0].slot.qualified_layout_name == INTO[ordinal].0
        && read.cells[0].slot.storage.get() == ordinal as u32
        && read.cells[0].offset == 0
        && read.cells[0].bytes == vec![b'A' + ordinal as u8; INTO[ordinal].1]
        && read.cells[1].slot.qualified_layout_name == RESPONSE
        && read.cells[2].slot.qualified_layout_name == RESPONSE2
        && read.cells[1].bytes == [0; 4]
        && read.cells[2].bytes == [0; 4]
        && read.eib == expected_eib
}

#[test]
fn every_byte_and_typed_scalar_corruption_refuses_independent_comparison() {
    let mut machine = machine(true);
    for ordinal in 0..3 {
        apply(&mut machine, ordinal);
    }
    finish(&mut machine);
    let mut archive = machine.take_read_copyback().unwrap().ok().unwrap();
    for (ordinal, read) in archive.reads.iter_mut().enumerate() {
        let expected = vec![b'A' + ordinal as u8; INTO[ordinal].1];
        let digest = read.effect.result_digest;
        assert!(independent_synthetic_storage_matches(read, ordinal));
        for byte in 0..expected.len() {
            read.cells[0].bytes[byte] ^= 1;
            assert!(!independent_synthetic_storage_matches(read, ordinal));
            assert_eq!(read.effect.result_digest, digest);
            read.cells[0].bytes[byte] ^= 1;
        }
        for cell in 1..3 {
            for byte in 0..4 {
                read.cells[cell].bytes[byte] = 1;
                assert!(!independent_synthetic_storage_matches(read, ordinal));
                read.cells[cell].bytes[byte] = 0;
            }
            read.cells[cell].bytes.push(0);
            assert!(!independent_synthetic_storage_matches(read, ordinal));
            read.cells[cell].bytes.pop();
        }
        for cell in 0..read.eib.len() {
            let original = read.eib[cell].1.clone();
            match &mut read.eib[cell].1 {
                CicsReadCopybackValue::Bytes(bytes) => bytes.push(1),
                CicsReadCopybackValue::Decimal { scale, .. } => *scale += 1,
            }
            assert!(!independent_synthetic_storage_matches(read, ordinal));
            read.eib[cell].1 = match &original {
                CicsReadCopybackValue::Bytes(_) => CicsReadCopybackValue::Decimal {
                    coefficient: 0,
                    scale: 0,
                },
                CicsReadCopybackValue::Decimal { .. } => CicsReadCopybackValue::Bytes(vec![0]),
            };
            assert!(!independent_synthetic_storage_matches(read, ordinal));
            read.eib[cell].1 = original;
        }
        assert!(independent_synthetic_storage_matches(read, ordinal));
    }
}

#[test]
fn reordered_duplicate_missing_and_extra_read_membership_refuse() {
    for order in [vec![1, 0, 2], vec![0, 0, 2], vec![0, 1], vec![0, 1, 2, 0]] {
        let mut machine = machine(true);
        for ordinal in order {
            apply(&mut machine, ordinal);
        }
        finish(&mut machine);
        assert!(matches!(machine.take_read_copyback(), Some(Err(_))));
    }
}

#[test]
fn budget_cap_plus_one_and_overflow_latch_without_runtime_failure() {
    let mut amount = 0;
    assert_eq!(charge(&mut amount, MAX_BYTES, MAX_BYTES), Ok(()));
    assert_eq!(
        charge(&mut amount, 1, MAX_BYTES),
        Err(CicsReadCopybackProblem::Bounds)
    );
    assert_eq!(amount, MAX_BYTES);
    let mut overflow = usize::MAX;
    assert_eq!(
        charge(&mut overflow, 1, usize::MAX),
        Err(CicsReadCopybackProblem::Bounds)
    );
    let mut observed = machine(true);
    observed.cics_read_copyback.as_mut().unwrap().bytes = MAX_BYTES;
    for ordinal in 0..3 {
        apply(&mut observed, ordinal);
    }
    finish(&mut observed);
    assert!(matches!(
        observed.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Bounds))
    ));
    assert_eq!(
        observed.variable("CUSTOMER-RECORD").unwrap().bytes(),
        vec![b'C'; 500]
    );
}

#[test]
fn snapshots_cannot_restore_a_live_archive_or_reconstruct_old_captures() {
    let mut machine = machine(true);
    apply(&mut machine, 0);
    let snapshot = machine.snapshot();
    machine.restore(snapshot).unwrap();
    assert!(matches!(
        machine.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Restoration))
    ));
    assert!(machine.take_read_copyback().is_none());
    machine.enable_read_copyback(plan(&machine)).unwrap();
    assert!(matches!(
        machine.take_read_copyback(),
        Some(Err(CicsReadCopybackProblem::Incomplete))
    ));
}

#[test]
fn structural_plan_refuses_wrong_layout_and_changed_resolved_views() {
    let mut foreign = machine(false);
    let mut wrong = plan(&foreign);
    wrong.reads[0].into.descriptor.qualified_layout_name = "USRSEC-RECORD".into();
    assert_eq!(
        foreign.enable_read_copyback(wrong),
        Err(CicsReadCopybackProblem::Layout)
    );
    let mut changed = machine(true);
    let effect = offer(&mut changed, 0);
    changed.views.get_mut("CARD-XREF-RECORD").unwrap().offset = 1;
    // Original application may refuse; either way the observer cannot publish.
    let _ = changed.resume_host(reply(
        effect.sequence,
        vec![b'A'; 50],
        CicsDisposition::Complete,
    ));
    assert!(matches!(changed.take_read_copyback(), Some(Err(_))));
}

#[test]
fn wrong_schema_and_non_normal_disposition_never_become_storage_witnesses() {
    for schema in [false, true] {
        let mut machine = machine(true);
        let effect = offer(&mut machine, 0);
        let mut result = reply(effect.sequence, vec![b'A'; 50], CicsDisposition::Complete);
        let Ok(HostResult::Cics(response)) = &mut result.outcome else {
            unreachable!();
        };
        if schema {
            response.payload =
                BoundedPayload::new("unrelated@1", vec![b'A'; 50], InvocationLimits::default())
                    .unwrap();
        } else {
            response.disposition = CicsDisposition::Ignored;
        }
        let _ = machine.resume_host(result);
        assert!(matches!(machine.take_read_copyback(), Some(Err(_))));
    }
}

#[test]
fn numeric_transaction_charges_actual_changed_type_before_copying() {
    let mut observed = machine(true);
    let mut plain = machine(false);
    for machine in [&mut observed, &mut plain] {
        machine
            .write_decimal_mode(
                "EIBTRNID",
                Decimal {
                    coefficient: 0,
                    scale: 0,
                },
                false,
            )
            .unwrap();
    }
    let effect = offer(&mut observed, 0);
    assert_eq!(effect, offer(&mut plain, 0));
    let mut result = reply(effect.sequence, vec![b'A'; 50], CicsDisposition::Complete);
    let Ok(HostResult::Cics(response)) = &mut result.outcome else {
        unreachable!();
    };
    response.transaction = "1234".into();
    assert_eq!(
        observed.resume_host(result.clone()),
        plain.resume_host(result)
    );
    assert_eq!(observed.snapshot(), plain.snapshot());
    let collector = observed.cics_read_copyback.as_ref().unwrap();
    // Three normal Decimal cells, FN2+AID1, and actual Decimal TRNID20 =83.
    assert_eq!(collector.bytes, 50 + 4 + 4 + 83);
    assert!(collector.incomplete.is_none());
    assert_eq!(collector.reads.len(), 1);
    assert!(
        collector.reads[0]
            .eib
            .iter()
            .any(|(name, value)| *name == "EIBTRNID"
                && *value
                    == CicsReadCopybackValue::Decimal {
                        coefficient: 1234,
                        scale: 0
                    })
    );
}

#[test]
fn actual_changed_type_cap_plus_one_refuses_without_copy_or_runtime_change() {
    let mut observed = machine(true);
    let mut plain = machine(false);
    for machine in [&mut observed, &mut plain] {
        machine
            .write_decimal_mode(
                "EIBTRNID",
                Decimal {
                    coefficient: 0,
                    scale: 0,
                },
                false,
            )
            .unwrap();
    }
    let width = 50 + 4 + 4 + 83;
    observed.cics_read_copyback.as_mut().unwrap().bytes = MAX_BYTES - width + 1;
    let effect = offer(&mut observed, 0);
    assert_eq!(effect, offer(&mut plain, 0));
    let mut result = reply(effect.sequence, vec![b'A'; 50], CicsDisposition::Complete);
    let Ok(HostResult::Cics(response)) = &mut result.outcome else {
        unreachable!();
    };
    response.transaction = "1234".into();
    assert_eq!(
        observed.resume_host(result.clone()),
        plain.resume_host(result)
    );
    assert_eq!(observed.snapshot(), plain.snapshot());
    let collector = observed.cics_read_copyback.as_ref().unwrap();
    assert_eq!(collector.bytes, MAX_BYTES - width + 1);
    assert!(collector.reads.is_empty());
    assert_eq!(collector.incomplete, Some(CicsReadCopybackProblem::Bounds));
}

#[test]
fn actual_read_occurrence_keys_survive_capture_without_mutation_metadata() {
    let mut machine = machine(true);
    let mut keys = Vec::new();
    for (ordinal, &(_, length)) in INTO.iter().enumerate() {
        let effect = offer(&mut machine, ordinal);
        assert!(!effect.request.is_mutating());
        assert!(effect.request.mutation().is_none());
        assert_eq!(effect.idempotency_key, Some(machine.effect_key().unwrap()));
        keys.push(effect.idempotency_key.clone().unwrap());
        machine
            .resume_host(reply(
                effect.sequence,
                vec![b'A' + ordinal as u8; length],
                CicsDisposition::Complete,
            ))
            .unwrap();
    }
    finish(&mut machine);
    let archive = machine.take_read_copyback().unwrap().ok().unwrap();
    for (read, key) in archive.reads().iter().zip(&keys) {
        assert_eq!(read.effect().key(), Some(key));
    }
}

#[test]
fn missing_or_foreign_read_occurrence_keys_refuse_without_changing_runtime() {
    for replacement in [
        None,
        Some(IdempotencyKey::new("foreign", InvocationLimits::default()).unwrap()),
    ] {
        let mut machine = machine(false);
        let mut collector = Collector::new(&machine, &plan(&machine)).unwrap();
        let effect = offer(&mut machine, 0);
        let snapshot = machine.snapshot();
        let mut changed = effect.clone();
        changed.idempotency_key = replacement;
        collector.offered(&machine, &changed, &machine.pending.as_ref().unwrap().kind);
        assert_eq!(collector.incomplete, Some(CicsReadCopybackProblem::Layout));
        assert!(collector.pending.is_none());
        assert!(collector.reads.is_empty());
        assert_eq!(machine.snapshot(), snapshot);
        machine
            .resume_host(reply(
                effect.sequence,
                vec![b'A'; 50],
                CicsDisposition::Complete,
            ))
            .unwrap();
        assert_eq!(
            machine.variable("CARD-XREF-RECORD").unwrap().bytes(),
            vec![b'A'; 50]
        );
    }
}
