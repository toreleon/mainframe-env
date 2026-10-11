use super::*;

fn inspect_machine(source: &str) -> ReferenceMachine {
    let mut machine = super::tests::divide_rounding_machine();
    for name in ["R", "Q"] {
        machine
            .write_decimal(
                name,
                Decimal {
                    coefficient: 0,
                    scale: 0,
                },
            )
            .unwrap();
    }
    let mut layout = machine.layouts["SRC"].clone();
    layout.category = LayoutCategory::Alphanumeric;
    layout.length = source.len();
    layout.element_length = source.len();
    machine.layouts.insert("SRC".into(), layout);
    machine.views.get_mut("SRC").unwrap().length = source.len();
    machine.write("SRC", source.as_bytes()).unwrap();
    machine
}

fn arguments(phrase: &str) -> Vec<String> {
    std::iter::once("SRC")
        .chain(phrase.split_whitespace())
        .map(str::to_owned)
        .collect()
}

#[test]
fn inspect_tallying_applies_every_phrase_and_counter_once() {
    // GnuCOBOL 3.2, cobc -x -std=ibm agrees on these results.
    for (source, phrase, first, second) in [
        ("SUSU", "TALLYING R FOR ALL 'S' ALL 'U'", 4, 0),
        ("SUSU", "TALLYING R FOR ALL 'S' Q FOR ALL 'U'", 2, 2),
        ("AAAA", "TALLYING R FOR ALL 'AA' Q FOR ALL 'A'", 2, 0),
        ("ABA", "TALLYING R FOR ALL 'A' Q FOR LEADING 'B'", 2, 0),
        (
            "S:U:S",
            "TALLYING R FOR ALL 'S' BEFORE INITIAL ':' ALL 'U' AFTER INITIAL ':'",
            2,
            0,
        ),
        ("SUUS", "TALLYING R FOR ALL 'S' 'U'", 4, 0),
    ] {
        let mut machine = inspect_machine(source);
        machine.inspect_op(&arguments(phrase)).unwrap();
        assert_eq!(
            machine.decimal("R").unwrap().coefficient,
            first * 100,
            "{phrase}"
        );
        assert_eq!(
            machine.decimal("Q").unwrap().coefficient,
            second * 100,
            "{phrase}"
        );
        assert_eq!(machine.read("SRC").unwrap(), source.as_bytes());
    }
}

#[test]
fn inspect_replacing_applies_every_phrase_without_reexamining_replacements() {
    // GnuCOBOL 3.2, cobc -x -std=ibm agrees on these results.
    for (source, phrase, expected) in [
        ("SUSU", "REPLACING ALL 'S' BY '1' ALL 'U' BY '1'", "1111"),
        (
            "ABABA",
            "REPLACING FIRST 'A' BY 'X' FIRST 'B' BY 'Y' ALL 'A' BY 'Z'",
            "XYZBZ",
        ),
        ("AAAA", "REPLACING ALL 'AA' BY 'BC' ALL 'A' BY 'Z'", "BCBC"),
        ("ABBA", "REPLACING ALL 'A' BY 'B' ALL 'B' BY 'C'", "BCCB"),
        (
            "AABACA",
            "REPLACING LEADING 'A' BY 'L' ALL 'B' BY 'X' CHARACTERS BY '0'",
            "LLX000",
        ),
        (
            "S:U:S",
            "REPLACING ALL 'S' BY '1' BEFORE INITIAL ':' ALL 'U' BY '2' AFTER INITIAL ':'",
            "1:2:S",
        ),
        ("SUUS", "REPLACING ALL 'S' BY '1' 'U' BY '2'", "1221"),
    ] {
        let mut machine = inspect_machine(source);
        machine.inspect_op(&arguments(phrase)).unwrap();
        assert_eq!(
            machine.read("SRC").unwrap(),
            expected.as_bytes(),
            "{phrase}"
        );
    }
}

#[test]
fn inspect_unparseable_trailing_phrase_is_rejected_before_mutation() {
    for phrase in [
        "REPLACING ALL 'S' BY '1' ALL 'U'",
        "REPLACING ALL 'S' BY '1' GARBAGE",
        "TALLYING R FOR ALL 'S' FIRST 'U'",
        "TALLYING R FOR ALL 'S' Q FOR",
    ] {
        let mut machine = inspect_machine("SUSU");
        let error = machine.inspect_op(&arguments(phrase)).expect_err(phrase);
        assert_eq!(
            error.execution_problem().category,
            FailureCategory::MalformedInput
        );
        assert_eq!(machine.read("SRC").unwrap(), b"SUSU");
        assert_eq!(machine.decimal("R").unwrap().coefficient, 0);
    }
}

#[test]
fn inspect_mixed_tallying_consumes_each_character_once() {
    // Pinned IBM 6.5 rlpsinspb comparison cycle: CHARACTERS is the fallback.
    // GnuCOBOL 3.2 returns 7 here; IBM's first-match consumption gives 6.
    let mut machine = inspect_machine("AABACA");
    machine
        .inspect_op(&arguments("TALLYING R FOR LEADING 'A' ALL 'B' CHARACTERS"))
        .unwrap();
    assert_eq!(machine.decimal("R").unwrap().coefficient, 600);
}
