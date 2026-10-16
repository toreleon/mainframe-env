//! Byte and failure-boundary controls for omitted reference-modification length.
use super::*;

fn tokens(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

fn fixture() -> ReferenceMachine {
    let mut m = ReferenceMachine::from_binary(
        &super::tests::binary(),
        super::tests::invocation(),
        CodecLimits::default(),
    )
    .unwrap();
    let base = m.bases.len();
    m.bases.push(vec![b'.'; 512]);
    for (name, parent, offset, length, element_length, occurs, category) in [
        ("TABLE", None, 168, 200, 20, 10, LayoutCategory::Group),
        (
            "TABLE.CODE",
            Some("TABLE"),
            168,
            4,
            4,
            1,
            LayoutCategory::Alphanumeric,
        ),
        (
            "TABLE.DESC",
            Some("TABLE"),
            172,
            16,
            16,
            1,
            LayoutCategory::Alphanumeric,
        ),
        ("OUT", None, 400, 20, 20, 1, LayoutCategory::Alphanumeric),
        ("OTHER", None, 420, 20, 20, 1, LayoutCategory::Alphanumeric),
    ] {
        let simple_name = name.rsplit('.').next().unwrap().to_string();
        m.views.insert(
            name.into(),
            StorageView {
                base,
                offset,
                length,
            },
        );
        m.simple_layouts
            .entry(simple_name.clone())
            .or_default()
            .push(name.into());
        m.layouts.insert(
            name.into(),
            LayoutMetadata {
                name: name.into(),
                simple_name,
                category,
                picture: String::new(),
                digits: 0,
                scale: 0,
                native_binary: false,
                signed: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                linkage: false,
                offset,
                length,
                element_length,
                occurs,
                occurs_min: occurs,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                dynamic: false,
                dynamic_limit: 0,
                parent: parent.map(str::to_string),
                alias_of: None,
                occurs_clause: occurs > 1,
                condition_values: Vec::new(),
                object_class: None,
            },
        );
    }
    m.bases[base][168..188].copy_from_slice(b"0000ABCDEFGHIJKLMNOP");
    m.bases[base][348..368].copy_from_slice(b"9999qrstuvwxyzABCDEF");
    m.bases[base][400..420].copy_from_slice(b"0000-???????????????");
    m.bases[base][420..440].copy_from_slice(b"other---------------");
    m.implicit.insert("IDX".into(), integer_value(1));
    m
}

#[test]
fn indexed_sender_to_omitted_length_receiver_preserves_prefix_and_truncates_right() {
    let mut m = fixture();
    m.move_op(&tokens(&[
        "DESC", "(", "IDX", ")", "TO", "OUT", "(", "6:", ")",
    ]))
    .unwrap();
    assert_eq!(m.read("OUT").unwrap(), b"0000-ABCDEFGHIJKLMNO");
    assert_eq!(m.read("OTHER").unwrap(), b"other---------------");
}

#[test]
fn omitted_length_sender_and_dynamic_start_read_the_complete_remaining_extent() {
    let mut m = fixture();
    m.implicit.insert("START".into(), integer_value(6));
    m.move_op(&tokens(&["OUT", "(", "START:", ")", "TO", "OTHER"]))
        .unwrap();
    assert_eq!(m.read("OTHER").unwrap(), b"???????????????     ");
    let reference = m.reference(&tokens(&["OUT", "(", "1:", ")"])).unwrap();
    assert_eq!(
        m.read_reference(&reference).unwrap(),
        b"0000-???????????????"
    );
}

#[test]
fn chained_selected_occurrence_uses_its_element_extent_before_omitted_length() {
    let mut m = fixture();
    m.implicit.insert("IDX".into(), integer_value(10));
    m.move_op(&tokens(&[
        "DESC", "(", "IDX", ")", "(", "6:", ")", "TO", "OUT", "(", "10:", ")",
    ]))
    .unwrap();
    assert_eq!(m.read("OUT").unwrap(), b"0000-????vwxyzABCDEF");
}

#[test]
fn invalid_modified_receiver_refuses_without_writing_its_bare_prefix() {
    for bound in ["0:", "22:", "20:2", "1:-1", "2:18446744073709551615"] {
        let mut m = fixture();
        let before = m.read("OUT").unwrap();
        assert_eq!(
            m.move_op(&tokens(&["'XYZ'", "TO", "OUT", "(", bound, ")"])),
            Err(MachineProblem::ReferenceModificationError),
            "{bound}",
        );
        assert_eq!(m.read("OUT").unwrap(), before, "{bound}");
    }
}

#[test]
fn invalid_later_receiver_keeps_prior_complete_receiver_write_order() {
    let mut m = fixture();
    assert_eq!(
        m.move_op(&tokens(&["'XYZ'", "TO", "OTHER", "OUT", "(", "0:", ")"])),
        Err(MachineProblem::ReferenceModificationError),
    );
    assert_eq!(m.read("OTHER").unwrap(), b"XYZ                 ");
    assert_eq!(m.read("OUT").unwrap(), b"0000-???????????????");
}

#[test]
fn explicit_zero_length_and_one_past_end_keep_existing_reference_bounds() {
    let m = fixture();
    for bounds in ["6:0", "21:0", "21:"] {
        let r = m.reference(&tokens(&["OUT", "(", bounds, ")"])).unwrap();
        assert!(m.read_reference(&r).unwrap().is_empty(), "{bounds}");
    }
    assert!(matches!(
        m.reference(&tokens(&["OUT", "(", "22:", ")"])),
        Err(MachineProblem::ReferenceModificationError),
    ));
}
