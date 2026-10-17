//! Synthetic live-machine controls; zero corpus, actor, replay or IBM credit.
use super::*;
use mainframe_env_host_api::{Db2Result, Db2Row};
use mainframe_env_ir::{IrLimits, ModuleBuilder, encode_binary};

const CELLS: [(&str, usize); 3] = [("ROW-OUT", 4), ("SQLERRD", 24), ("WSSTART", 2)];
fn machine() -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    for &(name, length) in &CELLS {
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
    let mut machine = ReferenceMachine::from_binary(
        &binary,
        super::super::tests::invocation(),
        CodecLimits::default(),
    )
    .unwrap();
    for &(name, length) in &CELLS {
        machine.layouts.insert(
            name.into(),
            LayoutMetadata {
                name: name.into(),
                simple_name: name.into(),
                category: LayoutCategory::Alphanumeric,
                picture: format!("X({length})"),
                digits: 0,
                scale: 0,
                native_binary: false,
                signed: false,
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
    machine.write_raw("ROW-OUT", b"BEFR").unwrap();
    machine.write_raw("SQLERRD", &[0x71; 24]).unwrap();
    machine.write_raw("WSSTART", b"02").unwrap();
    machine
}
fn slot(machine: &ReferenceMachine, name: &str, length: usize) -> Db2CopybackSlot {
    let storage = *machine
        .storage_names_by_id
        .iter()
        .find(|(_, actual)| actual.as_str() == name)
        .unwrap()
        .0;
    Db2CopybackSlot::new(storage, name.into(), length).unwrap()
}
fn plan(machine: &ReferenceMachine, operations: Vec<Db2Operation>) -> Db2CopybackPlan {
    Db2CopybackPlan::new(
        operations,
        vec![slot(machine, "ROW-OUT", 4), slot(machine, "SQLERRD", 24)],
    )
    .unwrap()
}
fn offer(machine: &mut ReferenceMachine, tokens: &[&str]) -> EffectRequest {
    let args = tokens
        .iter()
        .map(|token| (*token).to_string())
        .collect::<Vec<_>>();
    let Step::Effect(effect) = machine.sql_effect(&args).unwrap() else {
        panic!("actual SQL effect required")
    };
    *effect
}
fn reply(sequence: u64, code: i32, state: &str, columns: Option<Vec<Vec<u8>>>) -> EffectResult {
    EffectResult {
        sequence,
        outcome: Ok(HostResult::Db2(Db2Result {
            sqlcode: code,
            sqlstate: state.into(),
            message: String::new(),
            rows: columns
                .map(|columns| vec![Db2Row { columns }])
                .unwrap_or_default(),
            affected_rows: 7,
        })),
    }
}

#[test]
fn actual_fetch100_then_close0_preserves_both_postimages_and_unchanged_sqlerrd() {
    let mut observed = machine();
    let mut plain = machine();
    observed
        .enable_db2_copyback(plan(
            &observed,
            vec![Db2Operation::FetchCursor, Db2Operation::CloseCursor],
        ))
        .unwrap();
    for (tokens, code, state) in [
        (
            ["FETCH", "CUR1", "INTO", ":ROW-OUT"].as_slice(),
            100,
            "02000",
        ),
        (["CLOSE", "CUR1"].as_slice(), 0, "00000"),
    ] {
        let effect = offer(&mut observed, tokens);
        let other = offer(&mut plain, tokens);
        assert_eq!(effect, other);
        assert_eq!(
            observed.resume_host(reply(effect.sequence, code, state, None)),
            plain.resume_host(reply(effect.sequence, code, state, None))
        );
        assert_eq!(observed.snapshot(), plain.snapshot());
    }
    assert_eq!(
        observed.implicit.get("SQLCODE"),
        Some(&CobolValue::Decimal(Decimal {
            coefficient: 0,
            scale: 0
        }))
    );
    let archive = observed.take_db2_copyback().unwrap().ok().unwrap();
    assert_eq!(
        archive
            .records()
            .iter()
            .map(Db2CopybackRecord::provider_sqlcode)
            .collect::<Vec<_>>(),
        [100, 0]
    );
    for (record, code) in archive.records().iter().zip([100, 0]) {
        assert_eq!(record.cells()[0].bytes(), b"BEFR");
        assert_eq!(record.cells()[1].bytes(), [0x71; 24]);
        assert_eq!(record.provider_affected_rows(), 7);
        assert!(record.implicit().contains(&(
            "SQLCODE",
            Db2CopybackValue::Decimal {
                coefficient: code,
                scale: 0
            }
        )));
        assert!(record.key().is_some());
    }
    assert_eq!(archive.origin().execution(), &plain.invocation.execution_id);
    assert_eq!(
        archive.origin().principal(),
        plain.invocation.principal.id()
    );
}

#[test]
fn actual_sql_input_none_and_selected_row_bytes_survive_later_overwrite() {
    let mut machine = machine();
    machine
        .enable_db2_copyback(plan(&machine, vec![Db2Operation::Select]))
        .unwrap();
    let effect = offer(
        &mut machine,
        &[
            "SELECT", "VAL", "INTO", ":ROW-OUT", "FROM", "T", "WHERE", "KEY", ">=", ":WSSTART",
        ],
    );
    let HostRequest::Db2(request) = &effect.request else {
        panic!("Db2 request")
    };
    assert_eq!(request.inputs.len(), 1);
    assert_eq!(request.inputs["WSSTART"].indicator, None);
    assert_eq!(request.outputs, ["ROW-OUT"]);
    let result = reply(effect.sequence, 0, "00000", Some(vec![b"COPY".to_vec()]));
    let digest = canonical_result_digest(&result.outcome).unwrap();
    machine.resume_host(result).unwrap();
    machine.write_raw("ROW-OUT", b"AFTR").unwrap();
    let archive = machine.take_db2_copyback().unwrap().ok().unwrap();
    let record = &archive.records()[0];
    assert_eq!(record.input_count(), 1);
    assert_eq!(record.present_indicators(), 0);
    assert_eq!(record.cells()[0].bytes(), b"COPY");
    assert_eq!(record.result_digest(), &digest);
    assert_eq!(
        record.request_digest(),
        &canonical_request_digest(&effect.request).unwrap()
    );
    assert!(record.key().is_none());
    assert_eq!(machine.variable("ROW-OUT").unwrap().bytes(), b"AFTR");
}

#[test]
fn wrong_sequence_unknown_and_row_shape_preserve_original_refusals() {
    for variant in 0..3 {
        let mut observed = machine();
        let mut plain = machine();
        observed
            .enable_db2_copyback(plan(&observed, vec![Db2Operation::FetchCursor]))
            .unwrap();
        let effect = offer(&mut observed, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
        offer(&mut plain, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
        let result = match variant {
            0 => reply(effect.sequence + 1, 0, "00000", None),
            1 => EffectResult {
                sequence: effect.sequence,
                outcome: Err(HostProblem::UnknownOutcome),
            },
            _ => reply(effect.sequence, 0, "00000", Some(vec![])),
        };
        let left = observed.resume_host(result.clone());
        let right = plain.resume_host(result);
        assert!(left.is_err());
        assert_eq!(left, right);
        assert_eq!(observed.snapshot(), plain.snapshot());
        assert!(matches!(
            observed.take_db2_copyback().unwrap(),
            Err(Db2CopybackProblem::Application) | Err(Db2CopybackProblem::Order)
        ));
    }
}

#[test]
fn wrong_operation_refuses_only_observation_and_same_sql_outcome_remains() {
    let mut observed = machine();
    let mut plain = machine();
    observed
        .enable_db2_copyback(plan(&observed, vec![Db2Operation::Select]))
        .unwrap();
    let effect = offer(&mut observed, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
    offer(&mut plain, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
    let result = reply(effect.sequence, 100, "02000", None);
    assert_eq!(
        observed.resume_host(result.clone()),
        plain.resume_host(result)
    );
    assert_eq!(observed.snapshot(), plain.snapshot());
    assert!(matches!(
        observed.take_db2_copyback().unwrap(),
        Err(Db2CopybackProblem::Order)
    ));
}

#[test]
fn plan_bounds_layout_identity_and_pending_scope_refuse_before_capture() {
    let mut machine = machine();
    assert!(matches!(
        Db2CopybackPlan::new(vec![Db2Operation::Select; MAX_STATEMENTS + 1], vec![]),
        Err(Db2CopybackProblem::Bounds)
    ));
    let cell = slot(&machine, "ROW-OUT", MAX_BYTES);
    assert!(matches!(
        Db2CopybackPlan::new(vec![Db2Operation::Select], vec![cell]),
        Err(Db2CopybackProblem::Bounds)
    ));
    let cell = slot(&machine, "ROW-OUT", 5);
    assert_eq!(
        machine.enable_db2_copyback(
            Db2CopybackPlan::new(vec![Db2Operation::Select], vec![cell]).unwrap()
        ),
        Err(Db2CopybackProblem::Layout)
    );
    let valid = plan(&machine, vec![Db2Operation::FetchCursor]);
    offer(&mut machine, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
    assert_eq!(
        machine.enable_db2_copyback(valid),
        Err(Db2CopybackProblem::Scope)
    );
}

#[test]
fn runtime_view_replacement_refuses_no_storage_copy_and_restore_cannot_create_archive() {
    let mut observed = machine();
    let mut plain = machine();
    observed
        .enable_db2_copyback(plan(&observed, vec![Db2Operation::FetchCursor]))
        .unwrap();
    let effect = offer(&mut observed, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
    offer(&mut plain, &["FETCH", "CUR1", "INTO", ":ROW-OUT"]);
    let id = slot(&plain, "SQLERRD", 24).storage();
    observed.views_by_id.get_mut(&id).unwrap().length = 23;
    plain.views_by_id.get_mut(&id).unwrap().length = 23;
    let result = reply(effect.sequence, 100, "02000", None);
    assert_eq!(
        observed.resume_host(result.clone()),
        plain.resume_host(result)
    );
    assert_eq!(observed.snapshot(), plain.snapshot());
    assert!(observed.db2_copyback.as_ref().unwrap().records.is_empty());
    assert!(matches!(
        observed.take_db2_copyback().unwrap(),
        Err(Db2CopybackProblem::Layout)
    ));
    let mut restored = machine();
    let snapshot = restored.snapshot();
    restored
        .enable_db2_copyback(plan(&restored, vec![Db2Operation::Select]))
        .unwrap();
    restored.restore(snapshot).unwrap();
    assert!(matches!(
        restored.take_db2_copyback().unwrap(),
        Err(Db2CopybackProblem::Restoration)
    ));
}

#[test]
fn incomplete_trace_and_second_enable_refuse_without_completion_claim() {
    let mut machine = machine();
    let selected = plan(&machine, vec![Db2Operation::Select]);
    machine.enable_db2_copyback(selected.clone()).unwrap();
    assert_eq!(
        machine.enable_db2_copyback(selected),
        Err(Db2CopybackProblem::AlreadyEnabled)
    );
    assert!(matches!(
        machine.take_db2_copyback().unwrap(),
        Err(Db2CopybackProblem::Incomplete)
    ));
    assert!(machine.take_db2_copyback().is_none());
}
