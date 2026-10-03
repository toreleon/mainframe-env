//! Source-backed recognition only; mixed evaluation remains a source gap.
use mainframe_env_host_api::{ImsSsaBoolean, ImsSsaLimits, ImsSsaProblem, parse_ims_ssa};

fn fields(_: &str, field: &str) -> Option<usize> {
    match field {
        "KEY" => Some(2),
        "ONE" => Some(1),
        "FIVE" => Some(5),
        _ => None,
    }
}

#[test]
fn mixed_boolean_recognition_preserves_all_connector_identities() {
    let connectors = [
        (b'*', ImsSsaBoolean::DependentAnd),
        (b'&', ImsSsaBoolean::DependentAnd),
        (b'+', ImsSsaBoolean::LogicalOr),
        (b'|', ImsSsaBoolean::LogicalOr),
        (b'#', ImsSsaBoolean::IndependentAnd),
    ];
    for (first, first_kind) in connectors {
        for (second, second_kind) in connectors {
            let mut raw = b"ROOT    (KEY     GEA1".to_vec();
            raw.push(first);
            raw.extend(b"ONE     NE");
            raw.push(0);
            raw.push(second);
            raw.extend(b"ONE     LT");
            raw.extend([255, b')']);
            let parsed = parse_ims_ssa(&raw, ImsSsaLimits::default(), &fields).unwrap();
            assert_eq!(parsed.connectors, [first_kind, second_kind]);
            assert_eq!(parsed.predicates.len(), 3);
            assert_eq!(parsed.predicates[0].value, b"A1");
            assert_eq!(parsed.predicates[1].value, [0]);
            assert_eq!(parsed.predicates[2].value, [255]);
        }
    }
}

#[test]
fn mixed_boolean_binary_values_are_length_delimited_not_connector_scanned() {
    let parsed = parse_ims_ssa(
        b"ROOT    (FIVE    EQ*&+|##ONE     NE\0|KEY     LEB2)",
        ImsSsaLimits::default(),
        &fields,
    )
    .unwrap();
    assert_eq!(parsed.predicates[0].value, b"*&+|#");
    assert_eq!(
        parsed.connectors,
        [ImsSsaBoolean::IndependentAnd, ImsSsaBoolean::LogicalOr]
    );
    for raw in [
        b"ROOT    (KEY     EQA1&ONE     EQ)".as_slice(),
        b"ROOT    ((KEY     EQA1|KEY     EQB2)&ONE     EQX)".as_slice(),
        b"ROOT    (KEY     EQA1|KEY     EQB2)junk".as_slice(),
        b"ROOT    (KEY     EQA1&&ONE     EQX)".as_slice(),
    ] {
        assert!(parse_ims_ssa(raw, ImsSsaLimits::default(), &fields).is_err());
    }
    assert_eq!(
        parse_ims_ssa(
            b"ROOT    (FIVE    EQ&(X|#)",
            ImsSsaLimits::default(),
            &fields
        ),
        Err(ImsSsaProblem::ParenthesisInValue)
    );
}
