use super::*;
use mainframe_env_ir::{Effect, IrLimits, ModuleBuilder};

#[derive(Clone, Copy, Debug)]
struct Case {
    name: &'static str,
    category: LayoutCategory,
    signed: bool,
    digits: usize,
    scale: u32,
    initial: i128,
    verb: &'static str,
    arguments: &'static str,
    expected: i128,
}

fn machine_for(case: Case) -> ReferenceMachine {
    let length = match case.category {
        LayoutCategory::PackedDecimal => (case.digits + 2) / 2,
        _ => case.digits,
    };
    let mut builder = ModuleBuilder::new(IrLimits::default());
    builder.add_storage("N", length as u64, None).unwrap();
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
    let binary =
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
            .unwrap();
    let mut machine =
        ReferenceMachine::from_binary(&binary, super::tests::invocation(), CodecLimits::default())
            .unwrap();
    let layout = LayoutMetadata {
        name: "N".into(),
        simple_name: "N".into(),
        category: case.category,
        picture: String::new(),
        digits: case.digits,
        scale: case.scale,
        native_binary: false,
        signed: case.signed,
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
    };
    let initial = encode_decimal(
        &layout,
        Decimal {
            coefficient: case.initial,
            scale: case.scale,
        },
    )
    .unwrap();
    machine.layouts.insert("N".into(), layout);
    machine.simple_layouts.insert("N".into(), vec!["N".into()]);
    machine.write("N", &initial).unwrap();
    machine
}

fn stored(machine: &ReferenceMachine) -> Decimal {
    decode_decimal(machine.layout("N").unwrap(), &machine.read("N").unwrap()).unwrap()
}

#[test]
fn move_truncates_numeric_display_and_packed_receivers() {
    let cases = [
        Case {
            name: "signed display reproducer",
            category: LayoutCategory::NumericDisplay,
            signed: true,
            digits: 3,
            scale: 2,
            initial: 220,
            verb: "move",
            arguments: "73 TO N",
            expected: 300,
        },
        Case {
            name: "unsigned display reproducer",
            category: LayoutCategory::NumericDisplay,
            signed: false,
            digits: 6,
            scale: 5,
            initial: 50516,
            verb: "move",
            arguments: "49 TO N",
            expected: 900000,
        },
        Case {
            name: "negative signed display",
            category: LayoutCategory::NumericDisplay,
            signed: true,
            digits: 2,
            scale: 0,
            initial: -95,
            verb: "move",
            arguments: "-107 TO N",
            expected: -7,
        },
        Case {
            name: "signed packed",
            category: LayoutCategory::PackedDecimal,
            signed: true,
            digits: 2,
            scale: 0,
            initial: -95,
            verb: "move",
            arguments: "-107 TO N",
            expected: -7,
        },
        Case {
            name: "unsigned packed",
            category: LayoutCategory::PackedDecimal,
            signed: false,
            digits: 2,
            scale: 0,
            initial: 95,
            verb: "move",
            arguments: "107 TO N",
            expected: 7,
        },
    ];
    for case in cases {
        let mut machine = machine_for(case);
        let args = case
            .arguments
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(machine.move_op(&args), Ok(()), "{}", case.name);
        assert_eq!(stored(&machine).coefficient, case.expected, "{}", case.name);
    }
}

#[test]
fn arithmetic_overflow_truncates_without_handler_and_preserves_with_handler() {
    let categories = [
        ("signed display", LayoutCategory::NumericDisplay, true, -95),
        (
            "unsigned display",
            LayoutCategory::NumericDisplay,
            false,
            95,
        ),
        ("signed packed", LayoutCategory::PackedDecimal, true, -95),
        ("unsigned packed", LayoutCategory::PackedDecimal, false, 95),
    ];
    for (name, category, signed, initial) in categories {
        let result = if signed { -107 } else { 107 };
        let cases = [
            ("add", if signed { "-12 TO N" } else { "12 TO N" }),
            ("subtract", if signed { "12 FROM N" } else { "-12 FROM N" }),
            ("multiply", "2 BY N"),
            (
                "divide",
                if signed {
                    "-190 BY 1 GIVING N"
                } else {
                    "190 BY 1 GIVING N"
                },
            ),
            ("compute", if signed { "N = -190" } else { "N = 190" }),
        ];
        for (verb, arguments) in cases {
            let expected = if matches!(verb, "multiply" | "divide" | "compute") {
                if signed { -90 } else { 90 }
            } else {
                result % 100
            };
            for with_handler in [false, true] {
                let case = Case {
                    name,
                    category,
                    signed,
                    digits: 2,
                    scale: 0,
                    initial,
                    verb,
                    arguments,
                    expected,
                };
                let mut machine = machine_for(case);
                let args = arguments
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                assert_eq!(
                    machine.arithmetic(verb, &args, with_handler),
                    Ok(true),
                    "{name} {verb} handler={with_handler}"
                );
                assert_eq!(
                    stored(&machine).coefficient,
                    if with_handler { initial } else { expected },
                    "{name} {verb} handler={with_handler}"
                );
            }
        }
    }
}

#[test]
fn multiply_reproducers_truncate_signed_and_rounded_display_values() {
    let cases = [
        Case {
            name: "negative scaled",
            category: LayoutCategory::NumericDisplay,
            signed: true,
            digits: 6,
            scale: 3,
            initial: -653563,
            verb: "multiply",
            arguments: "19 BY N",
            expected: -417697,
        },
        Case {
            name: "rounded large",
            category: LayoutCategory::NumericDisplay,
            signed: false,
            digits: 16,
            scale: 4,
            initial: 9999999999999999,
            verb: "multiply",
            arguments: "20 BY N ROUNDED",
            expected: 9999999999999980,
        },
    ];
    for case in cases {
        let mut machine = machine_for(case);
        let args = case
            .arguments
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(
            machine.arithmetic(case.verb, &args, false),
            Ok(true),
            "{}",
            case.name
        );
        assert_eq!(stored(&machine).coefficient, case.expected, "{}", case.name);
    }
}

#[test]
fn size_error_phrase_runs_handler_and_preserves_display_receiver() {
    let case = Case {
        name: "handler",
        category: LayoutCategory::NumericDisplay,
        signed: false,
        digits: 2,
        scale: 0,
        initial: 95,
        verb: "add",
        arguments: "12 TO N",
        expected: 7,
    };
    let initial = machine_for(case);
    let mut builder = ModuleBuilder::new(IrLimits::default());
    builder.add_storage("N", 2, None).unwrap();
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    let mut attributes = case
        .arguments
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| (format!("arg_{index:03}"), Attribute::Text(token.into())))
        .collect::<BTreeMap<_, _>>();
    attributes.insert("control_node".into(), Attribute::Integer(0));
    attributes.insert("control_role".into(), Attribute::Text("statement".into()));
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "add", 1).unwrap(),
            Vec::new(),
            0,
            attributes,
            vec![Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition],
            Vec::new(),
            None,
        )
        .unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "control", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([
                ("control_node".into(), Attribute::Integer(1)),
                ("control_parent".into(), Attribute::Integer(0)),
                ("control_role".into(), Attribute::Text("branch".into())),
                (
                    "control_text".into(),
                    Attribute::Text("ON SIZE ERROR".into()),
                ),
                ("edge_branch_false".into(), Attribute::Integer(3)),
            ]),
            vec![Effect::ProgramControl, Effect::Condition],
            Vec::new(),
            None,
        )
        .unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "move", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([
                ("arg_000".into(), Attribute::Text("1".into())),
                ("arg_001".into(), Attribute::Text("TO".into())),
                ("arg_002".into(), Attribute::Text("RETURN-CODE".into())),
            ]),
            vec![Effect::MemoryWrite],
            Vec::new(),
            None,
        )
        .unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "control", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([
                ("control_node".into(), Attribute::Integer(3)),
                ("control_role".into(), Attribute::Text("terminator".into())),
            ]),
            vec![Effect::ProgramControl],
            Vec::new(),
            None,
        )
        .unwrap();
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
    let binary =
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
            .unwrap();
    let mut machine =
        ReferenceMachine::from_binary(&binary, super::tests::invocation(), CodecLimits::default())
            .unwrap();
    let initial_bytes = initial.read("N").unwrap();
    machine.layouts = initial.layouts;
    machine.simple_layouts = initial.simple_layouts;
    machine.write("N", &initial_bytes).unwrap();
    assert!(matches!(
        machine.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()),
        MachineDrive::Completed(_)
    ));
    assert_eq!(stored(&machine).coefficient, 95);
    assert_eq!(
        machine.implicit.get("RETURN-CODE"),
        Some(&CobolValue::Decimal(Decimal {
            coefficient: 1,
            scale: 0
        }))
    );
}
