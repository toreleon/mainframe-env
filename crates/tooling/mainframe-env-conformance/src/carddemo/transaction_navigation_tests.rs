//! Independent frozen public-source navigation controls.
use super::transaction_navigation::{self as navigation, Capture, IndexCase, NavigationCase};
use super::*;

// Complete independent literals frozen before the implementation; exactly 350 bytes each.
const RAW: [&str; 12] = [
    "0000000000000041010001ONLINE    NAV ROW 41                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-03                                    ",
    "0000000000000042010001ONLINE    NAV ROW 42                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-01                                    ",
    "0000000000000043010001ONLINE    NAV ROW 43                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-01                                    ",
    "0000000000000044010001ONLINE    NAV ROW 44                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-02                                    ",
    "0000000000000045010001ONLINE    NAV ROW 45                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-04                                    ",
    "0000000000000046010001ONLINE    NAV ROW 46                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-04                                    ",
    "0000000000000047010001ONLINE    NAV ROW 47                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-02                                    ",
    "0000000000000048010001ONLINE    NAV ROW 48                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-05                                    ",
    "0000000000000049010001ONLINE    NAV ROW 49                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-03                                    ",
    "0000000000000050010001ONLINE    NAV ROW 50                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-01                                    ",
    "0000000000000051010001ONLINE    NAV ROW 51                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-05                                    ",
    "0000000000000052010001ONLINE    NAV ROW 52                                                                                          0000000010{123456789NAVIGATION SHOP                                   BOSTON                                            02110     05000244537657402026-02-28                2026-03-02                                    ",
];

fn ebcdic(literal: &str) -> Vec<u8> {
    CodePage::Cp037.encode(literal, literal.len()).unwrap()
}
fn independently_expected(order: &[u8]) -> transaction_harness::RawRows {
    (
        order
            .iter()
            .map(|key| ebcdic(RAW[usize::from(*key - 41)]))
            .collect(),
        order
            .iter()
            .map(|key| ebcdic(&format!("{key:016}")))
            .collect(),
        3,
    )
}
fn state(capture: &Capture) {
    assert!(
        !capture.states.is_empty(),
        "actual state comparisons required"
    );
    let expected_primary =
        independently_expected(&[41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52]);
    let expected_index = independently_expected(&[42, 43, 50, 44, 47, 52, 41, 49, 45, 46, 48, 51]);
    let first = &capture.states[0];
    assert_eq!(first.len(), 5);
    assert_eq!(first[0], expected_primary);
    assert_eq!(first[1], expected_index);
    assert_eq!(
        first[2..].iter().map(|tuple| tuple.2).collect::<Vec<_>>(),
        vec![1, 1, 1]
    );
    for actual in &capture.states {
        assert_eq!(
            actual, first,
            "every actual stage must preserve full tuples"
        );
    }
    assert!(!capture.trace.iter().any(|e| matches!(
        e.operation,
        CicsOperation::Write | CicsOperation::Rewrite | CicsOperation::Delete
    )));
}
fn field(response: &serde_json::Value, name: &str, literal: &str) {
    let fields = online_screen_fields(response).unwrap();
    let actual = fields
        .get(name)
        .unwrap_or_else(|| panic!("missing actual field {name}"));
    assert!(
        actual == literal.as_bytes() || actual == &ebcdic(literal),
        "{name}: {actual:?}, expected {literal:?}"
    );
}
fn page(response: &serde_json::Value, keys: &[u8], number: &str, error: &str) {
    assert_eq!(response["mapset"], "COTRN00");
    assert_eq!(response["map"], "COTRN0A");
    field(response, "PGMNAME", "COTRN00C");
    field(response, "TRNNAME", "CT00");
    field(response, "PAGENUM", number);
    field(response, "ERRMSG", &format!("{error:<78}"));
    for i in 0..10 {
        let suffix = format!("{:02}", i + 1);
        if let Some(key) = keys.get(i) {
            field(response, &format!("TRNID{suffix}"), &format!("{key:016}"));
            field(response, &format!("TDATE{suffix}"), "02/28/26");
            field(
                response,
                &format!("TDESC{suffix}"),
                &format!("{:<26}", format!("NAV ROW {key}")),
            );
            field(response, &format!("TAMT{:03}", i + 1), "+00000001.00");
        } else {
            for (name, width) in [("TRNID", 16), ("TDATE", 8), ("TDESC", 26), ("TAMT0", 12)] {
                field(response, &format!("{name}{suffix}"), &" ".repeat(width));
            }
        }
    }
}
fn exact_menu(response: &serde_json::Value) {
    assert_eq!(response["mapset"], "COMEN01");
    assert_eq!(response["map"], "COMEN1A");
    field(
        response,
        "OPTN006",
        "06. Transaction List                    ",
    );
    field(
        response,
        "OPTN007",
        "07. Transaction View                    ",
    );
    field(
        response,
        "OPTN008",
        "08. Transaction Add                     ",
    );
}
fn exact_detail(response: &serde_json::Value, key: u8) {
    assert_eq!(response["mapset"], "COTRN01");
    assert_eq!(response["map"], "COTRN1A");
    field(response, "PGMNAME", "COTRN01C");
    field(response, "TRNNAME", "CT01");
    let fields = match key {
        42 => [
            ("TRNIDIN", "0000000000000042"),
            ("TRNID", "0000000000000042"),
            ("CARDNUM", "0500024453765740"),
            ("TTYPCD", "01"),
            ("TCATCD", "0001"),
            ("TRNSRC", "ONLINE    "),
            (
                "TDESC",
                "NAV ROW 42                                                  ",
            ),
            ("TRNAMT", "+00000001.00"),
            ("TORIGDT", "2026-02-28"),
            ("TPROCDT", "2026-03-01"),
            ("MID", "123456789"),
            ("MNAME", "NAVIGATION SHOP               "),
            ("MCITY", "BOSTON                   "),
            ("MZIP", "02110     "),
        ],
        43 => [
            ("TRNIDIN", "0000000000000043"),
            ("TRNID", "0000000000000043"),
            ("CARDNUM", "0500024453765740"),
            ("TTYPCD", "01"),
            ("TCATCD", "0001"),
            ("TRNSRC", "ONLINE    "),
            (
                "TDESC",
                "NAV ROW 43                                                  ",
            ),
            ("TRNAMT", "+00000001.00"),
            ("TORIGDT", "2026-02-28"),
            ("TPROCDT", "2026-03-01"),
            ("MID", "123456789"),
            ("MNAME", "NAVIGATION SHOP               "),
            ("MCITY", "BOSTON                   "),
            ("MZIP", "02110     "),
        ],
        52 => [
            ("TRNIDIN", "0000000000000052"),
            ("TRNID", "0000000000000052"),
            ("CARDNUM", "0500024453765740"),
            ("TTYPCD", "01"),
            ("TCATCD", "0001"),
            ("TRNSRC", "ONLINE    "),
            (
                "TDESC",
                "NAV ROW 52                                                  ",
            ),
            ("TRNAMT", "+00000001.00"),
            ("TORIGDT", "2026-02-28"),
            ("TPROCDT", "2026-03-02"),
            ("MID", "123456789"),
            ("MNAME", "NAVIGATION SHOP               "),
            ("MCITY", "BOSTON                   "),
            ("MZIP", "02110     "),
        ],
        _ => panic!("independent detail case not declared"),
    };
    for (name, literal) in fields {
        field(response, name, literal);
    }
    field(response, "ERRMSG", &" ".repeat(78));
}
fn native(capture: &Capture, at: usize, order: &[u8], eof: bool) {
    let start = &capture.probes[at];
    let cursor = start[0].as_str().expect("real native cursor");
    assert!(!cursor.is_empty());
    assert!(start[1].is_null() && start[2].is_null() && start[3].is_null());
    for (i, key) in order.iter().enumerate() {
        let raw = ebcdic(RAW[usize::from(*key - 41)]);
        assert_eq!(
            capture.probes[at + i + 1],
            serde_json::json!([cursor, raw, ebcdic(&format!("{key:016}")), raw[304..330]])
        );
    }
    if eof {
        assert_eq!(
            capture.probes[at + order.len() + 1],
            serde_json::json!([cursor, null, null, null])
        );
    }
}

#[tokio::test]
async fn menu_list_primary_first_page_exact_and_read_only() {
    let c = navigation::compare_case(NavigationCase::FirstPage)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    exact_menu(&c.screens[0]);
    page(
        &c.screens[1],
        &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50],
        "00000001",
        "",
    );
}

#[tokio::test]
async fn list_pf8_second_page_and_bottom_refusal_exact() {
    let c = navigation::compare_case(NavigationCase::Bottom)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    page(
        &c.screens[2],
        &[51, 52],
        "00000002",
        "You have reached the bottom of the page...",
    );
    page(
        &c.screens[3],
        &[51, 52],
        "00000002",
        "You are already at the bottom of the page...",
    );
}

#[tokio::test]
async fn list_pf7_back_and_top_refusal_exact() {
    let c = navigation::compare_case(NavigationCase::Previous)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    page(
        &c.screens[3],
        &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50],
        "00000001",
        "You have reached the top of the page...",
    );
    page(
        &c.screens[4],
        &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50],
        "00000001",
        "You are already at the top of the page...",
    );
    let starts = c
        .trace
        .iter()
        .enumerate()
        .filter(|(_, e)| e.operation == CicsOperation::StartBrowse)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 3, "first page, PF8 and first PF7 browse");
    let previous = &c.trace[starts[2]..];
    let end = previous
        .iter()
        .position(|e| e.operation == CicsOperation::EndBrowse)
        .expect("first PF7 must end its browse");
    let reads = previous[..=end]
        .iter()
        .filter(|e| e.operation == CicsOperation::ReadPrev)
        .collect::<Vec<_>>();
    assert_eq!(
        reads.len(),
        12,
        "discard, ten rows and mandatory lookbehind"
    );
    for read in &reads[..11] {
        assert_eq!(
            (
                read.outcome.as_str(),
                read.response,
                read.response2,
                read.payload_bytes
            ),
            ("NORMAL", 0, 0, 350)
        );
    }
    assert_eq!(reads[11].outcome, "ENDFILE");
    assert_eq!(reads[11].response, 20);
    assert_eq!(
        previous
            .iter()
            .filter(|e| e.operation == CicsOperation::EndBrowse)
            .count(),
        1
    );
    assert!(!previous[end + 1..].iter().any(|e| matches!(
        e.operation,
        CicsOperation::ReadNext | CicsOperation::ReadPrev
    )));
}

#[tokio::test]
async fn list_selection_s_and_lowercase_detail_exact() {
    let c = navigation::compare_case(NavigationCase::Selection)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    exact_detail(&c.screens[2], 42);
    exact_detail(&c.screens[5], 43);
}

#[tokio::test]
async fn detail_menu_lookup_all_fields_exact() {
    let c = navigation::compare_case(NavigationCase::Lookup)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    exact_detail(&c.screens[2], 52);
}

#[tokio::test]
async fn detail_pf3_returns_actual_list_or_menu() {
    let c = navigation::compare_case(NavigationCase::Back)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    page(
        &c.screens[3],
        &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50],
        "00000001",
        "",
    );
    exact_menu(&c.screens[6]);
}

#[tokio::test]
async fn detail_pf4_clears_and_pf5_browses_exact() {
    let c = navigation::compare_case(NavigationCase::Clear)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    for (name, width) in [
        ("TRNIDIN", 16),
        ("TRNID", 16),
        ("CARDNUM", 16),
        ("TTYPCD", 2),
        ("TCATCD", 4),
        ("TRNSRC", 10),
        ("TDESC", 60),
        ("TRNAMT", 12),
        ("TORIGDT", 10),
        ("TPROCDT", 10),
        ("MID", 9),
        ("MNAME", 30),
        ("MCITY", 25),
        ("MZIP", 10),
        ("ERRMSG", 78),
    ] {
        field(&c.screens[3], name, &" ".repeat(width));
    }
    page(
        &c.screens[4],
        &[42, 43, 44, 45, 46, 47, 48, 49, 50, 51],
        "00000001",
        "",
    );
    let at = c
        .trace
        .iter()
        .rposition(|e| e.operation == CicsOperation::Xctl)
        .expect("PF5 must XCTL to the list");
    let browse = &c.trace[at..];
    for operation in [CicsOperation::StartBrowse, CicsOperation::EndBrowse] {
        assert_eq!(
            browse.iter().filter(|e| e.operation == operation).count(),
            1
        );
    }
    let reads = browse
        .iter()
        .filter(|e| e.operation == CicsOperation::ReadNext)
        .collect::<Vec<_>>();
    assert_eq!(reads.len(), 12, "discard41, rows42..51 and lookahead52");
    for read in reads {
        assert_eq!(
            (
                read.outcome.as_str(),
                read.response,
                read.response2,
                read.payload_bytes
            ),
            ("NORMAL", 0, 0, 350)
        );
    }
}

#[tokio::test]
async fn detail_missing_and_empty_refuse_without_mutation() {
    let c = navigation::compare_case(NavigationCase::DetailRefusal)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    field(
        &c.screens[2],
        "ERRMSG",
        "Transaction ID NOT found...                                                   ",
    );
    field(
        &c.screens[5],
        "ERRMSG",
        "Tran ID can NOT be empty...                                                   ",
    );
    for (name, width) in [
        ("TRNID", 16),
        ("CARDNUM", 16),
        ("TTYPCD", 2),
        ("TCATCD", 4),
        ("TRNSRC", 10),
        ("TDESC", 60),
        ("TRNAMT", 12),
        ("TORIGDT", 10),
        ("TPROCDT", 10),
        ("MID", 9),
        ("MNAME", 30),
        ("MCITY", 25),
        ("MZIP", 10),
    ] {
        field(&c.screens[2], name, &" ".repeat(width));
        // Initial LOW-VALUES and the fresh task's reentered PIC X spaces are distinct.
        field(&c.screens[4], name, &"\0".repeat(width));
        field(&c.screens[5], name, &" ".repeat(width));
    }
}

#[tokio::test]
async fn list_invalid_numeric_selection_and_aid_refuse_without_mutation() {
    let c = navigation::compare_case(NavigationCase::ListRefusal)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    for (i, error) in [
        (2, "Tran ID must be Numeric ..."),
        (5, "Invalid selection. Valid value is S"),
        (8, "Invalid key pressed. Please see below..."),
    ] {
        page(
            &c.screens[i],
            &[41, 42, 43, 44, 45, 46, 47, 48, 49, 50],
            "00000001",
            error,
        );
    }
}

#[tokio::test]
async fn add_pf3_returns_menu_without_inserting() {
    let c = navigation::compare_case(NavigationCase::AddBack)
        .await
        .expect("actual selected navigation prerequisite");
    state(&c);
    assert_eq!(c.screens[1]["mapset"], "COTRN02");
    assert_eq!(c.screens[1]["map"], "COTRN2A");
    exact_menu(&c.screens[2]);
}

#[tokio::test]
async fn aix_forward_duplicate_keys_keep_complete_record_identity() {
    let c = navigation::compare_index(IndexCase::Forward)
        .await
        .expect("actual native forward prerequisite");
    state(&c);
    native(
        &c,
        0,
        &[42, 43, 50, 44, 47, 52, 41, 49, 45, 46, 48, 51],
        true,
    );
}
#[tokio::test]
async fn aix_reverse_and_positioned_duplicate_group_exact() {
    let c = navigation::compare_index(IndexCase::ReversePositioned)
        .await
        .expect("actual native reverse prerequisite");
    state(&c);
    native(
        &c,
        0,
        &[51, 48, 46, 45, 49, 41, 52, 47, 44, 50, 43, 42],
        true,
    );
    native(&c, 15, &[42, 43, 50, 44], false);
}
#[tokio::test]
async fn aix_ended_cursor_refuses_without_mutation() {
    let c = navigation::compare_index(IndexCase::Ended)
        .await
        .expect("actual ended native cursor prerequisite");
    state(&c);
    native(&c, 0, &[42], false);
    let refusal = c.probes.last().unwrap().as_str().unwrap();
    assert!(
        refusal.contains("INVREQ") && refusal.contains("response: 16"),
        "actual typed refusal {refusal}"
    );
}
#[tokio::test]
async fn navigation_comparison_mismatch_emits_no_observation() {
    let result = navigation::compare_case(NavigationCase::WrongPrimaryOrder)
        .await
        .map(|_| ());
    let original = result
        .as_ref()
        .expect_err("real primary-order comparator must refuse")
        .clone();
    assert_eq!(original.code, "carddemo.navigation.comparison_mismatch");
    assert!(
        original.detail.starts_with("field TDESC01:"),
        "wrong-order comparison prerequisite: {original:?}"
    );
    let mut observations = RouteObservations::default();
    let problem =
        transaction_harness::bind_comparison(&mut observations, "screen navigation", result)
            .unwrap_err();
    assert_eq!(problem, original);
    assert!(observations.journeys().is_empty());
    assert!(observations.issues().is_empty());
}
#[tokio::test]
async fn screen_navigation_binding_requires_all_actual_steps() {
    let mut observations = RouteObservations::default();
    online_receipt::exercise_navigation_observations(&mut observations)
        .await
        .expect("all actual comparisons and shutdown are transport prerequisites");
    assert_eq!(
        observations.journeys(),
        [("CD.J06".into(), vec!["screen navigation".into()])],
        "missing actual screen-navigation observation"
    );
    assert!(observations.issues().is_empty());
}
#[tokio::test]
async fn aix_provider_probe_does_not_manufacture_application_observation() {
    let observations = RouteObservations::default();
    let forward = navigation::compare_index(IndexCase::Forward)
        .await
        .expect("actual AIX probe prerequisite");
    let reverse = navigation::compare_index(IndexCase::ReversePositioned)
        .await
        .expect("actual reverse probe prerequisite");
    state(&forward);
    state(&reverse);
    assert!(observations.journeys().is_empty());
    assert!(observations.issues().is_empty());
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../conformance/profiles/carddemo/workloads/carddemo-journeys.json"
    )))
    .unwrap();
    assert_eq!(manifest["status"], "planned_not_executed");
    assert!(close_carddemo_journeys(&manifest, observations.journeys()).is_err());
}
