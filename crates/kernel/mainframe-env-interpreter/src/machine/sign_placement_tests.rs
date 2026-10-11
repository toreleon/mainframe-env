use super::*;
use mainframe_env_ir::{IrLimits, ModuleBuilder, cobol_layout_definition_identity, encode_binary};

fn sign_machine(leading: bool, separate: bool) -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let length = 6 + u64::from(separate);
    builder.add_storage("R", length, None).unwrap();
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    builder
        .add_operation(
            block,
            cobol_layout_definition_identity(),
            Vec::new(),
            0,
            BTreeMap::from([
                ("name".into(), Attribute::Text("R".into())),
                ("simple_name".into(), Attribute::Text("R".into())),
                ("category".into(), Attribute::Text("numeric_display".into())),
                ("picture".into(), Attribute::Text("S9(6)".into())),
                ("digits".into(), Attribute::Integer(6)),
                ("scale".into(), Attribute::Integer(0)),
                ("signed".into(), Attribute::Integer(1)),
                (
                    "sign_leading".into(),
                    Attribute::Integer(i64::from(leading)),
                ),
                (
                    "sign_separate".into(),
                    Attribute::Integer(i64::from(separate)),
                ),
                ("section".into(), Attribute::Text("working".into())),
                ("offset".into(), Attribute::Integer(0)),
                ("length".into(), Attribute::Integer(length as i64)),
                ("element_length".into(), Attribute::Integer(length as i64)),
                ("occurs".into(), Attribute::Integer(1)),
                ("parent".into(), Attribute::Text(String::new())),
                ("condition_values".into(), Attribute::Text(String::new())),
            ]),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
    builder
        .add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "display", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([("arg_000".into(), Attribute::Text("R".into()))]),
            vec![
                mainframe_env_ir::Effect::MemoryRead,
                mainframe_env_ir::Effect::TerminalWrite,
            ],
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
    let binary = encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap();
    ReferenceMachine::from_binary(&binary, super::tests::invocation(), CodecLimits::default())
        .unwrap()
}

fn tokens(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).into()).collect()
}

#[test]
fn sign_leading_and_trailing_store_display_and_numeric_round_trip() {
    // IBM Enterprise COBOL 6.5 SIGN clause, rlddesig.html, baseline 2026-05-31.
    // Non-separate signs use the runtime's existing ASCII overpunch representation.
    for (leading, separate, expected) in [
        (true, true, ["+000007", "-000002", "+000000"]),
        (false, true, ["000007+", "000002-", "000000+"]),
        (true, false, ["{00007", "}00002", "{00000"]),
        (false, false, ["00000G", "00000K", "00000{"]),
    ] {
        for (value, bytes) in ["7", "-2", "0"].into_iter().zip(expected) {
            let mut machine = sign_machine(leading, separate);
            machine.move_op(&tokens(&[value, "TO", "R"])).unwrap();
            assert_eq!(
                machine.read("R").unwrap(),
                bytes.as_bytes(),
                "storage: leading={leading}, separate={separate}, value={value}"
            );
            let reference = machine.reference(&tokens(&["R"])).unwrap();
            assert_eq!(
                machine.display_reference(&reference).unwrap(),
                bytes.as_bytes()
            );
            let display = machine
                .operations
                .iter()
                .find(|op| op.identity.name() == "display")
                .unwrap()
                .clone();
            machine.execute(&display).unwrap();
            assert_eq!(machine.output, format!("{bytes}\n").as_bytes());
            assert!(machine.eval_condition(&tokens(&["R", "=", value])).unwrap());

            // A REDEFINES alphanumeric view shares the same bytes in both directions.
            let mut raw = machine.layouts["R"].clone();
            raw.name = "RAW".into();
            raw.simple_name = "RAW".into();
            raw.category = LayoutCategory::Alphanumeric;
            raw.alias_of = Some("R".into());
            machine.layouts.insert("RAW".into(), raw);
            machine
                .views
                .insert("RAW".into(), machine.views["R"].clone());
            assert_eq!(machine.read("RAW").unwrap(), bytes.as_bytes());
            machine.write("RAW", bytes.as_bytes()).unwrap();

            let edited = super::tests::edited_test_layout("+999999", 6, 0, false);
            let base = machine.bases.len();
            machine.bases.push(vec![b' '; 7]);
            machine.views.insert(
                "EDITED".into(),
                StorageView {
                    base,
                    offset: 0,
                    length: 7,
                },
            );
            machine.layouts.insert("EDITED".into(), edited);
            machine.move_op(&tokens(&["R", "TO", "EDITED"])).unwrap();
            let expected_edited = match value {
                "7" => "+000007",
                "-2" => "-000002",
                _ => "+000000",
            };
            assert_eq!(machine.read("EDITED").unwrap(), expected_edited.as_bytes());
            machine.move_op(&tokens(&["EDITED", "TO", "R"])).unwrap();
            assert_eq!(machine.read("R").unwrap(), bytes.as_bytes());
            machine
                .arithmetic("compute", &tokens(&["R", "=", "R", "+", "1"]), false)
                .unwrap();
            let incremented = (value.parse::<i128>().unwrap() + 1).to_string();
            assert!(
                machine
                    .eval_condition(&tokens(&["R", "=", &incremented]))
                    .unwrap()
            );
            machine
                .arithmetic("compute", &tokens(&["R", "=", "R", "-", "1"]), false)
                .unwrap();
            assert_eq!(machine.read("R").unwrap(), bytes.as_bytes());
        }
    }
}
