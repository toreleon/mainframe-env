//! Independent actual-route controls and missing-observation regressions.
use super::transaction_harness::{
    TransactionFixture, exercise_transaction_dates, exercise_transaction_duplicate,
};
use super::*;

const TRANSACT: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS";
const TRANSACT_AIX: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH";
const USRSEC: &str = "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS";
const CARDXREF: &str = "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS";
const CXACAIX: &str = "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH";

// Literal field expectations from CVTRA05Y, not derived from a program/handler result.
fn independent_record(key: &str, description: &str, origin: &str, processing: &str) -> Vec<u8> {
    let fields = [
        (key, 16),
        ("01", 2),
        ("0001", 4),
        ("ONLINE", 10),
        (description, 100),
        ("0000000010{", 11),
        ("123456789", 9),
        ("BOUNDARY SHOP", 50),
        ("BOSTON", 50),
        ("02110", 10),
        ("0500024453765740", 16),
        (origin, 26),
        (processing, 26),
        ("", 20),
    ];
    let mut text = String::new();
    for (literal, width) in fields {
        assert!(literal.len() <= width);
        text.push_str(literal);
        text.extend(std::iter::repeat_n(' ', width - literal.len()));
    }
    assert_eq!(text.len(), 350);
    CodePage::Cp037.encode(&text, 350).unwrap()
}

fn read_raw(server: &ProductServer, name: &str) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, u64) {
    match server
        .dataset_service()
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new(name, 128).unwrap(),
            member: None,
            key: None,
            max_records: 4096,
            control: Default::default(),
        })
        .unwrap()
    {
        DatasetResult::Records {
            records,
            identities,
            version,
        } => (records, identities, version),
        other => panic!("expected actual dataset rows, received {other:?}"),
    }
}

fn exact_error(response: &serde_json::Value, literal: &str) {
    assert_eq!(response["mapset"], "COTRN02");
    let fields = online_screen_fields(response).unwrap();
    let value = fields.get("ERRMSG").expect("actual ERRMSG field");
    let mut ascii = literal.as_bytes().to_vec();
    ascii.resize(78, b' ');
    let ebcdic = CodePage::Cp037
        .encode(std::str::from_utf8(&ascii).unwrap(), 78)
        .unwrap();
    // The existing terminal API preserves opaque map bytes; both exact encodings
    // preserve this literal and its complete padding. No substring acceptance.
    assert!(value == &ascii || value == &ebcdic, "ERRMSG {value:?}");
}

async fn invalid_date(origin: &str, processing: &str, expected: &str) {
    let baseline = independent_record(
        "0000000000000041",
        "BASELINE TRANSACTION",
        "2026-02-28",
        "2026-02-28",
    );
    let fixture = TransactionFixture::open_with_transaction_rows(vec![baseline.clone()])
        .await
        .unwrap();
    let route = select_regular_option(&fixture.server, &fixture.app, 8, "COTRN02")
        .await
        .unwrap();
    let before = read_raw(&fixture.server, TRANSACT);
    let index_before = read_raw(&fixture.server, TRANSACT_AIX);
    let users_before = read_raw(&fixture.server, USRSEC);
    let xref_before = read_raw(&fixture.server, CARDXREF);
    let xref_index_before = read_raw(&fixture.server, CXACAIX);
    assert_eq!(before.0, vec![baseline]);
    assert_eq!(
        before.1,
        vec![CodePage::Cp037.encode("0000000000000041", 16).unwrap()]
    );
    assert_eq!(before.2, 3);
    assert_eq!(index_before, before);
    let trace_before = fixture.server.online_trace(&route.session).unwrap().len();
    let response = carddemo_terminal_exchange(
        &fixture.app,
        &route.session,
        &route.headers,
        0x7d,
        independent_fields(origin, processing),
    )
    .await
    .unwrap();
    exact_error(&response, expected);
    assert_eq!(read_raw(&fixture.server, TRANSACT), before);
    assert_eq!(read_raw(&fixture.server, TRANSACT_AIX), index_before);
    assert_eq!(read_raw(&fixture.server, USRSEC), users_before);
    assert_eq!(read_raw(&fixture.server, CARDXREF), xref_before);
    assert_eq!(read_raw(&fixture.server, CXACAIX), xref_index_before);
    let trace = fixture.server.online_trace(&route.session).unwrap();
    let observed = &trace[trace_before..];
    assert!(
        observed
            .iter()
            .any(|entry| entry.operation == CicsOperation::Read && entry.outcome == "NORMAL")
    );
    assert!(
        !observed
            .iter()
            .any(|entry| entry.operation == CicsOperation::Write)
    );
    fixture
        .export_case(
            &response,
            &[
                before,
                index_before,
                users_before,
                xref_before,
                xref_index_before,
            ],
            &[
                read_raw(&fixture.server, TRANSACT),
                read_raw(&fixture.server, TRANSACT_AIX),
                read_raw(&fixture.server, USRSEC),
                read_raw(&fixture.server, CARDXREF),
                read_raw(&fixture.server, CXACAIX),
            ],
            &trace,
        )
        .unwrap();
    fixture.finish(Ok(())).await.unwrap();
}

fn independent_fields(origin: &str, processing: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ACTIDIN".into(), "00000000050".into()),
        ("TTYPCD".into(), "01".into()),
        ("TCATCD".into(), "0001".into()),
        ("TRNSRC".into(), "ONLINE".into()),
        ("TDESC".into(), "DATE BOUNDARY PURCHASE".into()),
        ("TRNAMT".into(), "+00000001.00".into()),
        ("TORIGDT".into(), origin.into()),
        ("TPROCDT".into(), processing.into()),
        ("MID".into(), "123456789".into()),
        ("MNAME".into(), "BOUNDARY SHOP".into()),
        ("MCITY".into(), "BOSTON".into()),
        ("MZIP".into(), "02110".into()),
        ("CONFIRM".into(), "Y".into()),
    ])
}

async fn valid_boundary(origin: &str, processing: &str) {
    let baseline = independent_record(
        "0000000000000041",
        "BASELINE TRANSACTION",
        "2026-02-28",
        "2026-02-28",
    );
    let fixture = TransactionFixture::open_with_transaction_rows(vec![baseline.clone()])
        .await
        .unwrap();
    let route = select_regular_option(&fixture.server, &fixture.app, 8, "COTRN02")
        .await
        .unwrap();
    let before = read_raw(&fixture.server, TRANSACT);
    let index_before = read_raw(&fixture.server, TRANSACT_AIX);
    let users_before = read_raw(&fixture.server, USRSEC);
    let xref_before = read_raw(&fixture.server, CARDXREF);
    let xref_index_before = read_raw(&fixture.server, CXACAIX);
    let baseline_identity = CodePage::Cp037.encode("0000000000000041", 16).unwrap();
    let inserted_identity = CodePage::Cp037.encode("0000000000000042", 16).unwrap();
    assert_eq!(
        before,
        (vec![baseline.clone()], vec![baseline_identity.clone()], 3)
    );
    assert_eq!(index_before, before);
    let trace_before = fixture.server.online_trace(&route.session).unwrap().len();
    let response = carddemo_terminal_exchange(
        &fixture.app,
        &route.session,
        &route.headers,
        0x7d,
        independent_fields(origin, processing),
    )
    .await
    .unwrap();
    exact_error(
        &response,
        "Transaction added successfully.  Your Tran ID is 0000000000000042.",
    );
    let expected = independent_record(
        "0000000000000042",
        "DATE BOUNDARY PURCHASE",
        origin,
        processing,
    );
    assert_eq!(
        read_raw(&fixture.server, TRANSACT).0,
        vec![baseline.clone(), expected.clone()]
    );
    assert_eq!(
        read_raw(&fixture.server, TRANSACT),
        (
            vec![baseline.clone(), expected.clone()],
            vec![baseline_identity.clone(), inserted_identity.clone()],
            4,
        )
    );
    let index_expected = if processing < "2026-02-28" {
        vec![expected, baseline]
    } else {
        vec![baseline, expected]
    };
    assert_eq!(read_raw(&fixture.server, TRANSACT_AIX).0, index_expected);
    let index_identities = if processing < "2026-02-28" {
        vec![inserted_identity, baseline_identity]
    } else {
        vec![baseline_identity, inserted_identity]
    };
    assert_eq!(
        read_raw(&fixture.server, TRANSACT_AIX),
        (index_expected, index_identities, 4)
    );
    assert_eq!(read_raw(&fixture.server, USRSEC), users_before);
    assert_eq!(read_raw(&fixture.server, CARDXREF), xref_before);
    assert_eq!(read_raw(&fixture.server, CXACAIX), xref_index_before);
    let trace = fixture.server.online_trace(&route.session).unwrap();
    assert_eq!(
        trace[trace_before..]
            .iter()
            .filter(|entry| entry.operation == CicsOperation::Write && entry.outcome == "NORMAL")
            .count(),
        1
    );
    fixture
        .export_case(
            &response,
            &[
                before,
                index_before,
                users_before,
                xref_before,
                xref_index_before,
            ],
            &[
                read_raw(&fixture.server, TRANSACT),
                read_raw(&fixture.server, TRANSACT_AIX),
                read_raw(&fixture.server, USRSEC),
                read_raw(&fixture.server, CARDXREF),
                read_raw(&fixture.server, CXACAIX),
            ],
            &trace,
        )
        .unwrap();
    fixture.finish(Ok(())).await.unwrap();
}

async fn duplicate_collision() {
    let zero = independent_record(
        "0000000000000000",
        "RETAINED ZERO KEY",
        "2026-02-28",
        "2026-02-28",
    );
    let maximum = independent_record(
        "9999999999999999",
        "RETAINED MAXIMUM KEY",
        "2026-02-28",
        "2026-02-28",
    );
    let fixture =
        TransactionFixture::open_with_transaction_rows(vec![zero.clone(), maximum.clone()])
            .await
            .unwrap();
    let route = select_regular_option(&fixture.server, &fixture.app, 8, "COTRN02")
        .await
        .unwrap();
    let before = read_raw(&fixture.server, TRANSACT);
    let index_before = read_raw(&fixture.server, TRANSACT_AIX);
    let users_before = read_raw(&fixture.server, USRSEC);
    let xref_before = read_raw(&fixture.server, CARDXREF);
    let xref_index_before = read_raw(&fixture.server, CXACAIX);
    assert_eq!(before.0, vec![zero, maximum]);
    assert_eq!(
        before.1,
        vec![
            CodePage::Cp037.encode("0000000000000000", 16).unwrap(),
            CodePage::Cp037.encode("9999999999999999", 16).unwrap(),
        ]
    );
    assert_eq!(before.2, 3);
    assert_eq!(index_before, before);
    let trace_before = fixture.server.online_trace(&route.session).unwrap().len();
    let response = carddemo_terminal_exchange(
        &fixture.app,
        &route.session,
        &route.headers,
        0x7d,
        independent_fields("2026-02-28", "2026-02-28"),
    )
    .await
    .unwrap();
    exact_error(&response, "Tran ID already exist...");
    assert_eq!(read_raw(&fixture.server, TRANSACT), before);
    assert_eq!(read_raw(&fixture.server, TRANSACT_AIX), index_before);
    assert_eq!(read_raw(&fixture.server, USRSEC), users_before);
    assert_eq!(read_raw(&fixture.server, CARDXREF), xref_before);
    assert_eq!(read_raw(&fixture.server, CXACAIX), xref_index_before);
    let trace = fixture.server.online_trace(&route.session).unwrap();
    let observed = &trace[trace_before..];
    let file_order = observed
        .iter()
        .filter(|entry| {
            matches!(
                entry.operation,
                CicsOperation::StartBrowse
                    | CicsOperation::ReadPrev
                    | CicsOperation::EndBrowse
                    | CicsOperation::Write
            )
        })
        .map(|entry| (entry.operation, entry.outcome.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        file_order,
        vec![
            (CicsOperation::StartBrowse, "NORMAL"),
            (CicsOperation::ReadPrev, "NORMAL"),
            (CicsOperation::EndBrowse, "NORMAL"),
            (CicsOperation::Write, "DUPREC")
        ]
    );
    assert_eq!(
        observed
            .iter()
            .filter(|entry| entry.operation == CicsOperation::Write && entry.outcome == "DUPREC")
            .count(),
        1
    );
    let duplicate = observed
        .iter()
        .find(|entry| entry.operation == CicsOperation::Write && entry.outcome == "DUPREC")
        .unwrap();
    assert_eq!((duplicate.response, duplicate.response2), (14, 0));
    assert!(
        !observed
            .iter()
            .any(|entry| entry.operation == CicsOperation::Write && entry.outcome == "NORMAL")
    );
    fixture
        .export_case(
            &response,
            &[
                before,
                index_before,
                users_before,
                xref_before,
                xref_index_before,
            ],
            &[
                read_raw(&fixture.server, TRANSACT),
                read_raw(&fixture.server, TRANSACT_AIX),
                read_raw(&fixture.server, USRSEC),
                read_raw(&fixture.server, CARDXREF),
                read_raw(&fixture.server, CXACAIX),
            ],
            &trace,
        )
        .unwrap();
    fixture.finish(Ok(())).await.unwrap();
}

#[tokio::test]
async fn origin_format_refuses() {
    invalid_date(
        "2026/02/28",
        "2026-02-28",
        "Orig Date should be in format YYYY-MM-DD",
    )
    .await;
}
#[tokio::test]
async fn processing_format_refuses() {
    invalid_date(
        "2026-02-28",
        "2026/02/28",
        "Proc Date should be in format YYYY-MM-DD",
    )
    .await;
}
#[tokio::test]
async fn origin_month_refuses() {
    invalid_date(
        "2026-13-01",
        "2026-02-28",
        "Orig Date - Not a valid date...",
    )
    .await;
}
#[tokio::test]
async fn processing_month_refuses() {
    invalid_date(
        "2026-02-28",
        "2026-13-01",
        "Proc Date - Not a valid date...",
    )
    .await;
}
#[tokio::test]
async fn origin_nonleap_refuses() {
    invalid_date(
        "2026-02-29",
        "2026-02-28",
        "Orig Date - Not a valid date...",
    )
    .await;
}
#[tokio::test]
async fn processing_nonleap_refuses() {
    invalid_date(
        "2026-02-28",
        "2026-02-29",
        "Proc Date - Not a valid date...",
    )
    .await;
}
#[tokio::test]
async fn origin_century_refuses() {
    invalid_date(
        "1900-02-29",
        "2026-02-28",
        "Orig Date - Not a valid date...",
    )
    .await;
}
#[tokio::test]
async fn processing_century_refuses() {
    invalid_date(
        "2026-02-28",
        "1900-02-29",
        "Proc Date - Not a valid date...",
    )
    .await;
}
#[tokio::test]
async fn valid_nonleap_boundary_inserts() {
    valid_boundary("2026-02-28", "2026-03-01").await;
}
#[tokio::test]
async fn valid_leap_boundary_inserts() {
    valid_boundary("2024-02-29", "2024-02-29").await;
}
#[tokio::test]
async fn valid_century_boundary_inserts() {
    valid_boundary("2000-02-29", "2000-02-29").await;
}
#[tokio::test]
async fn occupied_zero_key_refuses_actual_write() {
    duplicate_collision().await;
}

// Binding controls retain the exact independent expectations from the accepted initial red.
#[tokio::test]
async fn date_validation_binding_requires_complete_actual_cases() {
    let mut observed = RouteObservations::default();
    exercise_transaction_dates(&mut observed).await.unwrap();
    assert_eq!(
        observed.journeys(),
        [("CD.J06".into(), vec!["date validation".into()])]
    );
}

#[tokio::test]
async fn duplicate_binding_requires_real_collision_and_no_mutation() {
    let mut observed = RouteObservations::default();
    exercise_transaction_duplicate(&mut observed).await.unwrap();
    assert_eq!(
        observed.journeys(),
        [("CD.J06".into(), vec!["duplicate condition".into()])]
    );
}

#[tokio::test]
async fn actual_screen_mismatch_refuses_binding() {
    let mut observations = RouteObservations::default();
    // The real format refusal must not satisfy the independent calendar-error comparison.
    let result = transaction_harness::compare_date(
        "2026/02/28",
        "2026-02-28",
        Some("Orig Date - Not a valid date..."),
    )
    .await;
    let original = result.as_ref().err().unwrap().clone();
    let problem =
        transaction_harness::bind_comparison(&mut observations, "date validation", result)
            .unwrap_err();
    assert_eq!(problem, original);
    assert_eq!(problem.code, "carddemo.transaction.observation_mismatch");
    assert!(observations.journeys().is_empty());
    assert!(observations.issues().is_empty());
}

#[tokio::test]
async fn successful_finish_stops_actual_server_before_artifact_cleanup() {
    let fixture = TransactionFixture::open_with_transaction_rows(vec![independent_record(
        "0000000000000041",
        "BASELINE TRANSACTION",
        "2026-02-28",
        "2026-02-28",
    )])
    .await
    .unwrap();
    select_regular_option(&fixture.server, &fixture.app, 8, "COTRN02")
        .await
        .unwrap();
    let server = fixture.server.clone();
    let app = fixture.app.clone();
    let artifact_path = fixture.artifact_path().to_path_buf();
    assert!(artifact_path.is_dir());
    fixture.finish(Ok(())).await.unwrap();
    assert!(!artifact_path.exists());
    assert!(!server.readiness().accepting);
    assert!(
        select_regular_option(&server, &app, 8, "COTRN02")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn actual_route_comparison_error_is_preserved_through_shutdown() {
    let fixture = TransactionFixture::open_with_transaction_rows(vec![independent_record(
        "0000000000000041",
        "BASELINE TRANSACTION",
        "2026-02-28",
        "2026-02-28",
    )])
    .await
    .unwrap();
    // Actual option 8 presents COTRN02, so expecting COMEN01 must refuse.
    let original = select_regular_option(&fixture.server, &fixture.app, 8, "COMEN01")
        .await
        .err()
        .unwrap();
    let server = fixture.server.clone();
    let app = fixture.app.clone();
    let artifact_path = fixture.artifact_path().to_path_buf();
    assert!(artifact_path.is_dir());
    let actual = fixture
        .finish::<()>(Err(original.clone()))
        .await
        .unwrap_err();
    assert_eq!(actual, original);
    assert!(!artifact_path.exists());
    assert!(!server.readiness().accepting);
    assert!(
        select_regular_option(&server, &app, 8, "COTRN02")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn online_receipt_binding_transports_only_actual_requirements_and_refuses_full_closure() {
    let mut observations = RouteObservations::default();
    online_receipt::exercise_transaction_observations(&mut observations)
        .await
        .unwrap();
    assert_eq!(
        observations.journeys(),
        [(
            "CD.J06".into(),
            vec!["date validation".into(), "duplicate condition".into(),]
        )]
    );
    assert!(observations.issues().is_empty());
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../conformance/profiles/carddemo/workloads/carddemo-journeys.json",
    )))
    .unwrap();
    assert_eq!(manifest["status"], "planned_not_executed");
    assert!(close_carddemo_journeys(&manifest, observations.journeys()).is_err());
}
