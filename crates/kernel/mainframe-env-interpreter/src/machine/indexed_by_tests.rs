use super::*;
use mainframe_env_ir::{IrLimits, ModuleBuilder};

fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

fn fixture() -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let table = builder.add_storage("TABLE-X", 8, None).unwrap();
    builder
        .add_storage(
            "TABLE-X.VALUE-X",
            2,
            Some(mainframe_env_ir::StorageReference {
                storage: table,
                offset: 0,
                length: 2,
            }),
        )
        .unwrap();
    let table_y = builder.add_storage("TABLE-Y", 12, None).unwrap();
    builder.add_storage("COUNT-X", 1, None).unwrap();
    builder
        .add_storage(
            "TABLE-Y.VALUE-Y",
            3,
            Some(mainframe_env_ir::StorageReference {
                storage: table_y,
                offset: 0,
                length: 3,
            }),
        )
        .unwrap();
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    for (name, category, parent, length, occurs, indexes) in [
        ("TABLE-X", "group", "", 8, 4, "IDX\u{1f}OTHER-IDX"),
        ("TABLE-X.VALUE-X", "alphanumeric", "TABLE-X", 2, 1, ""),
        ("TABLE-Y", "group", "", 12, 4, "Y-IDX"),
        ("TABLE-Y.VALUE-Y", "alphanumeric", "TABLE-Y", 3, 1, ""),
        ("COUNT-X", "numeric_display", "", 1, 1, ""),
    ] {
        builder
            .add_operation(
                block,
                OperationIdentity::new(NAMESPACE, "define", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("name".into(), Attribute::Text(name.into())),
                    (
                        "simple_name".into(),
                        Attribute::Text(name.rsplit('.').next().unwrap().into()),
                    ),
                    ("category".into(), Attribute::Text(category.into())),
                    (
                        "picture".into(),
                        Attribute::Text(if category == "group" {
                            String::new()
                        } else {
                            if category == "numeric_display" {
                                "9"
                            } else {
                                "X"
                            }
                            .repeat(length as usize)
                        }),
                    ),
                    (
                        "digits".into(),
                        Attribute::Integer(if category == "numeric_display" {
                            length
                        } else {
                            0
                        }),
                    ),
                    ("scale".into(), Attribute::Integer(0)),
                    ("signed".into(), Attribute::Integer(0)),
                    ("sign_separate".into(), Attribute::Integer(0)),
                    ("section".into(), Attribute::Text("working".into())),
                    ("offset".into(), Attribute::Integer(0)),
                    ("length".into(), Attribute::Integer(length)),
                    ("element_length".into(), Attribute::Integer(length / occurs)),
                    ("occurs".into(), Attribute::Integer(occurs)),
                    ("occurs_min".into(), Attribute::Integer(occurs)),
                    (
                        "occurs_clause".into(),
                        Attribute::Integer(i64::from(occurs > 1)),
                    ),
                    ("parent".into(), Attribute::Text(parent.into())),
                    ("indexes".into(), Attribute::Text(indexes.into())),
                    ("condition_values".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
    }
    for (name, attributes) in [
        (
            "search",
            BTreeMap::from([("control_node".into(), Attribute::Integer(7))]),
        ),
        (
            "control",
            BTreeMap::from([
                ("control_parent".into(), Attribute::Integer(7)),
                ("control_role".into(), Attribute::Text("branch".into())),
                (
                    "control_text".into(),
                    Attribute::Text("WHEN VALUE-X ( IDX ) = 'BB'".into()),
                ),
            ]),
        ),
        ("halt", BTreeMap::new()),
    ] {
        builder
            .add_operation(
                block,
                OperationIdentity::new(NAMESPACE, name, 1).unwrap(),
                Vec::new(),
                0,
                attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
    }
    let binary =
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
            .unwrap();
    let mut machine =
        ReferenceMachine::from_binary(&binary, super::tests::invocation(), CodecLimits::default())
            .unwrap();
    machine.write("TABLE-X", b"AABBCCDD").unwrap();
    machine.write("TABLE-Y", b"111222333444").unwrap();
    machine.write("COUNT-X", b"0").unwrap();
    machine
}

fn occurrence(machine: &ReferenceMachine, name: &str) -> i128 {
    value_decimal(machine.eval_value(&tokens(name)).unwrap())
        .unwrap()
        .coefficient
}

fn search(machine: &mut ReferenceMachine, all: bool) {
    let operation = machine
        .operations
        .iter()
        .find(|op| op.identity.name() == "search")
        .unwrap()
        .clone();
    machine
        .prepare_search(
            &operation,
            &tokens(if all { "ALL TABLE-X" } else { "TABLE-X" }),
        )
        .unwrap();
}

#[test]
fn set_indexed_by_to_selects_occurrence_without_a_storage_layout() {
    let mut machine = fixture();
    assert!(machine.layout("IDX").is_none());
    machine.set_op(&tokens("IDX TO 3")).unwrap();
    assert_eq!(occurrence(&machine, "IDX"), 3);
    let reference = machine.reference(&tokens("VALUE-X ( IDX )")).unwrap();
    assert_eq!(machine.read_reference(&reference).unwrap(), b"CC");
}

#[test]
fn set_indexed_by_up_and_down_adjust_occurrences() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX TO 1")).unwrap();
    machine.set_op(&tokens("IDX UP BY 2")).unwrap();
    assert_eq!(occurrence(&machine, "IDX"), 3);
    machine.set_op(&tokens("IDX DOWN BY 1")).unwrap();
    assert_eq!(occurrence(&machine, "IDX"), 2);
    // SET format 2 requires a nonzero literal, but permits a zero integer data item.
    machine.set_op(&tokens("IDX UP BY COUNT-X")).unwrap();
    assert_eq!(occurrence(&machine, "IDX"), 2);
}

#[test]
fn set_multiple_indexed_by_names_and_copy_an_index() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX OTHER-IDX TO 2")).unwrap();
    machine.set_op(&tokens("OTHER-IDX UP BY 1")).unwrap();
    assert_eq!(occurrence(&machine, "IDX"), 2);
    assert_eq!(occurrence(&machine, "OTHER-IDX"), 3);
    machine.set_op(&tokens("IDX TO OTHER-IDX")).unwrap();
    assert_eq!(occurrence(&machine, "IDX"), 3);
    assert!(machine.eval_condition(&tokens("IDX = OTHER-IDX")).unwrap());
    assert!(machine.eval_condition(&tokens("IDX > 2")).unwrap());
}

#[test]
fn serial_search_starts_at_the_set_index_and_reaches_at_end() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX TO 3")).unwrap();
    search(&mut machine, false);
    assert_eq!(machine.search_results.get(&7), Some(&false));
    machine.set_op(&tokens("IDX TO 2")).unwrap();
    search(&mut machine, false);
    assert_eq!(machine.search_results.get(&7), Some(&true));
    assert_eq!(occurrence(&machine, "IDX"), 2);
}

#[test]
fn search_all_initializes_the_index_independently_of_set() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX TO 4")).unwrap();
    search(&mut machine, true);
    assert_eq!(machine.search_results.get(&7), Some(&true));
    assert_eq!(occurrence(&machine, "IDX"), 2);
}

#[test]
fn indexed_by_subscripts_use_the_defining_tables_displacement() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX TO 4")).unwrap();
    let reference = machine.reference(&tokens("VALUE-Y ( IDX )")).unwrap();
    // IDX's displacement is (4 - 1) * 2 = 6, the third three-byte Y element.
    assert_eq!(machine.read_reference(&reference).unwrap(), b"333");
    machine.set_op(&tokens("Y-IDX TO IDX")).unwrap();
    let reference = machine.reference(&tokens("VALUE-Y ( Y-IDX )")).unwrap();
    // SET converts the sending occurrence to the receiver's table stride.
    assert_eq!(machine.read_reference(&reference).unwrap(), b"444");
    assert!(machine.eval_condition(&tokens("IDX = Y-IDX")).unwrap());
}

#[test]
fn indexed_by_set_rejects_invalid_values_and_keeps_indexes_unchanged() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX TO 2")).unwrap();
    for statement in [
        "IDX TO 0",
        "IDX TO -1",
        "IDX TO 1.5",
        "IDX UP BY 0",
        "IDX DOWN BY 2",
        "IDX UP BY 1.5",
        "IDX MISSING TO 3",
    ] {
        assert!(machine.set_op(&tokens(statement)).is_err(), "{statement}");
        assert_eq!(occurrence(&machine, "IDX"), 2, "{statement}");
    }
    machine.set_op(&tokens("IDX TO 5")).unwrap();
    assert_eq!(
        machine.reference(&tokens("VALUE-X ( IDX )")).unwrap_err(),
        MachineProblem::SubscriptError
    );
}

#[test]
fn indexed_by_checkpoint_preserves_set_and_search_position() {
    let mut machine = fixture();
    machine.set_op(&tokens("IDX TO 3")).unwrap();
    machine.set_op(&tokens("OTHER-IDX TO 2")).unwrap();
    let snapshot = machine.snapshot();
    assert_eq!(
        snapshot.implicit_values.get("IDX"),
        Some(&MachineSnapshotValue::Decimal {
            coefficient: 3,
            scale: 0
        })
    );
    let mut restored = fixture();
    restored.restore(snapshot).unwrap();
    assert_eq!(occurrence(&restored, "OTHER-IDX"), 2);
    search(&mut restored, false);
    assert_eq!(restored.search_results.get(&7), Some(&false));
}
