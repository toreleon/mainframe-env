//! Independent complete SQL row and maintenance-screen comparisons.
//! No new route, failure injection, SQLCA claim or closure token.
use super::*;

type Rows = Vec<Vec<Vec<u8>>>;

const TYPE_LITERALS: [(&str, &str); 7] = [
    ("01", "PURCHASE"),
    ("02", "PAYMENT"),
    ("03", "CREDIT"),
    ("04", "AUTHORIZATION"),
    ("05", "REFUND"),
    ("06", "REVERAL"),
    ("07", "ADJUSTMENT"),
];

const CATEGORY_LITERALS: [(&str, &str, &str); 18] = [
    ("01", "0001", "REGULAR SALES DRAFT"),
    ("01", "0002", "REGULAR CASH ADVANCE"),
    ("01", "0003", "CONVENIENCE CHECK DEBIT"),
    ("01", "0004", "ATM CASH ADVANCE"),
    ("01", "0005", "INTEREST AMOUNT"),
    ("02", "0001", "CASH PAYMENT"),
    ("02", "0002", "ELECTRONIC PAYMENT"),
    ("02", "0003", "CHECK PAYMENT"),
    ("03", "0001", "CREDIT TO ACCOUNT"),
    ("03", "0002", "CREDIT TO PURCHASE BALANCE"),
    ("03", "0003", "CREDIT TO CASH BALANCE"),
    ("04", "0001", "ZERO DOLLAR AUTHORIZATION"),
    ("04", "0002", "ONLINE PURCHASE AUTHORIZATION"),
    ("04", "0003", "TRAVEL BOOKING AUTHORIZATION"),
    ("05", "0001", "REFUND CREDIT"),
    ("06", "0001", "FRAUD REVERSAL"),
    ("06", "0002", "NON FRAUD REVERSAL"),
    ("07", "0001", "SALES DRAFT CREDIT ADJUSTMENT"),
];

const SOURCE_PINS: &[(&str, &str)] = &[
    (
        "app/app-transaction-type-db2/cbl/COTRTUPC.cbl",
        "c16e40c391c0ad2d3e797a0e01144ef5b78d3b0db0a1771411a95a9d08b01f67",
    ),
    (
        "app/app-transaction-type-db2/cbl/COTRTLIC.cbl",
        "916a5fe2279ad626147990d8f86fac1e4b296f827889485b020cc310b5323601",
    ),
    (
        "app/app-transaction-type-db2/csd/CRDDEMOD.csd",
        "d9f1bd3b2a9bf546c5a04e41a30c0c70d48197922e3e6b40f023f3356f906438",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTTYP.ctl",
        "65d7c9b86b82d6b85bf50bfa18740c40353104148d1c3ce33dafc1a1a114fd6f",
    ),
    (
        "app/app-transaction-type-db2/ctl/DB2LTCAT.ctl",
        "1e0d56c17d52a71b36e7e52c261cb992394ae6f66be35e0f223b030ae5354602",
    ),
    (
        "app/app-transaction-type-db2/dcl/DCLTRTYP.dcl",
        "4c61f60bdf03ba3cdca9a45eeaae21b645640df270751c884f350db3a8ad2677",
    ),
    (
        "app/app-transaction-type-db2/dcl/DCLTRCAT.dcl",
        "54bc32fd11e1d01b1c3be36ea5c6d75aa6af9231ae9e933017fefcb3e7fda7b1",
    ),
    (
        "app/app-transaction-type-db2/bms/COTRTUP.bms",
        "1496a35f219e92b5339e2651a58dabeea97a0410d7cf465a8658b207dfd1706c",
    ),
    (
        "app/app-transaction-type-db2/bms/COTRTLI.bms",
        "5867235e382d6fc54da67ad8dfbcc7db7e1183e999068789b6e310c5c0d18abf",
    ),
    (
        "app/app-transaction-type-db2/cpy-bms/COTRTUP.cpy",
        "36f1acf35569976b97aa4ae3bf8307c36c32d444b007b5f70c148f73601e5a11",
    ),
    (
        "app/app-transaction-type-db2/cpy-bms/COTRTLI.cpy",
        "ece821940dafaf827eb1e3a1720aa9cfad43e3f1e0b42968ed322473edde1616",
    ),
    (
        "app/cbl/COADM01C.cbl",
        "4e49afda5f685f3ebeb95ed04a93cf88df93d39c66d8d7f9ba2b9a3e00a1f690",
    ),
    (
        "app/cpy/COADM02Y.cpy",
        "86d4b5b5c89449126b2a37bb46458fe451a2523490d3c7f7f9c35d36199fe6b3",
    ),
    (
        "app/bms/COADM01.bms",
        "44a3f209e7fa8445355ab7ba687b3ac6e32ef4dfec225e04f4eb2bd57e39386a",
    ),
    (
        "app/cpy-bms/COADM01.CPY",
        "ced4c34fdb3fa4284561d83ff8e273cef2a1b45cefc36e56c34da6fec5528006",
    ),
];

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Seed,
    Inserted,
    Updated,
    Deleted,
}

fn expected_rows(phase: Phase) -> (Rows, Rows) {
    let mut types = TYPE_LITERALS
        .iter()
        .map(|(key, description)| vec![key.as_bytes().to_vec(), description.as_bytes().to_vec()])
        .collect::<Rows>();
    match phase {
        Phase::Inserted => types.push(vec![b"99".to_vec(), b"ONLINE SPECIAL".to_vec()]),
        Phase::Updated => types.push(vec![b"99".to_vec(), b"ONLINE UPDATED".to_vec()]),
        Phase::Seed | Phase::Deleted => {}
    }
    let categories = CATEGORY_LITERALS
        .iter()
        .map(|(key, category, description)| {
            vec![
                key.as_bytes().to_vec(),
                category.as_bytes().to_vec(),
                description.as_bytes().to_vec(),
            ]
        })
        .collect();
    (types, categories)
}

fn rows_match(phase: Phase, types: &Rows, categories: &Rows) -> bool {
    let expected = expected_rows(phase);
    types == &expected.0 && categories == &expected.1
}

pub(super) fn check_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (relative, expected) in SOURCE_PINS {
        let bytes = read_corpus_file(corpus, &corpus.join(relative))?;
        if format!("{:x}", Sha256::digest(&bytes)) != *expected {
            return Err(CorpusProblem::new(
                "carddemo.db2.online_source_drift",
                *relative,
            ));
        }
    }
    Ok(())
}

pub(super) fn require_rows(server: &ProductServer, phase: Phase) -> Result<(), CorpusProblem> {
    // table_rows returns committed raw columns in primary-key order. Never use
    // a previous provider read as an expected seed or normalize actual bytes.
    let types = server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?;
    let categories = server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE_CATEGORY")
        .map_err(terminal_problem)?;
    if !rows_match(phase, &types, &categories) {
        return Err(CorpusProblem::new(
            "carddemo.db2.online_rows_drift",
            "complete ordered type/category rows differ from pinned controls and the selected app action",
        ));
    }
    Ok(())
}

// Source WS-INFO-MSG/WS-STRING-OUT are 40 bytes, COMPUTE has no ROUNDED,
// and BMS/CPY INFOMSG is 45 bytes. Freeze all left/right padding. These
// comparisons select the current UTF-8 source/local symbolic-map transport;
// they make no claim of native IBM character-set behavior.
const COMMITTED_INFO: &[u8] = b"     Changes committed to database           ";
const DELETED_INFO: &[u8] = b"           Delete successful.                ";
const MISSING_ERROR: &[u8] =
    b"No record found for this key in database                                      ";
const EMPTY_ERROR: &[u8] =
    b"                                                                              ";

fn screen_matches(
    response: &serde_json::Value,
    information: Option<&[u8]>,
    error: &[u8],
) -> Result<bool, CorpusProblem> {
    let fields = online_screen_fields(response)?;
    Ok(response["mapset"] == "COTRTUP"
        && response["map"] == "CTRTUPA"
        && fields
            .get("PGMNAME")
            .is_some_and(|value| value == b"COTRTUPC")
        && fields.get("TRNNAME").is_some_and(|value| value == b"CTTU")
        && fields.get("ERRMSG").is_some_and(|value| value == error)
        && information
            .is_none_or(|expected| fields.get("INFOMSG").is_some_and(|value| value == expected)))
}

// Failure-only summaries preserve every exact predicate above. No actual screen
// values or customer data are published: only fixed field names, sizes, hashes
// and the first unequal byte offset leave this comparison.
fn mismatch_summary(name: &str, actual: Option<&[u8]>, expected: &[u8]) -> Option<String> {
    let Some(actual) = actual else {
        return Some(format!(
            "{name}: missing or invalid type; expected_length={}",
            expected.len()
        ));
    };
    if actual == expected {
        return None;
    }
    let first_difference = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected)
        .unwrap_or(actual.len().min(expected.len()));
    Some(format!(
        "{name}: expected_length={}; actual_length={}; expected_sha256={:x}; \
         actual_sha256={:x}; first_difference={first_difference}",
        expected.len(),
        actual.len(),
        Sha256::digest(expected),
        Sha256::digest(actual),
    ))
}

fn screen_diagnostic(
    response: &serde_json::Value,
    information: Option<&[u8]>,
    error: &[u8],
) -> Result<String, CorpusProblem> {
    let fields = online_screen_fields(response)?;
    let mut mismatches = Vec::new();
    for (name, expected) in [("mapset", &b"COTRTUP"[..]), ("map", &b"CTRTUPA"[..])] {
        if let Some(summary) =
            mismatch_summary(name, response[name].as_str().map(str::as_bytes), expected)
        {
            mismatches.push(summary);
        }
    }
    for (name, expected) in [
        ("PGMNAME", &b"COTRTUPC"[..]),
        ("TRNNAME", &b"CTTU"[..]),
        ("ERRMSG", error),
    ] {
        if let Some(summary) = mismatch_summary(name, fields.get(name).map(Vec::as_slice), expected)
        {
            mismatches.push(summary);
        }
    }
    if let Some(summary) = information.and_then(|expected| {
        mismatch_summary(
            "INFOMSG",
            fields.get("INFOMSG").map(Vec::as_slice),
            expected,
        )
    }) {
        mismatches.push(summary);
    }
    Ok(mismatches.join("; "))
}

pub(super) fn require_missing(response: &serde_json::Value) -> Result<(), CorpusProblem> {
    if !screen_matches(response, None, MISSING_ERROR)? {
        return Err(CorpusProblem::new(
            "carddemo.db2.online_missing_screen_drift",
            format!(
                "missing-key route lacks the exact source/map error bytes and program/transaction fields: {}",
                screen_diagnostic(response, None, MISSING_ERROR)?,
            ),
        ));
    }
    Ok(())
}

pub(super) fn require_committed(
    response: &serde_json::Value,
    phase: Phase,
) -> Result<(), CorpusProblem> {
    let info = match phase {
        Phase::Inserted | Phase::Updated => COMMITTED_INFO,
        Phase::Deleted => DELETED_INFO,
        Phase::Seed => {
            return Err(CorpusProblem::new(
                "carddemo.db2.online_phase_invalid",
                "seed phase is not an app commit",
            ));
        }
    };
    if !screen_matches(response, Some(info), EMPTY_ERROR)? {
        return Err(CorpusProblem::new(
            "carddemo.db2.online_commit_screen_drift",
            format!(
                "app commit lacks exact source/map success and error fields: {}",
                screen_diagnostic(response, Some(info), EMPTY_ERROR)?,
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(info: &[u8], error: &[u8]) -> serde_json::Value {
        let mut bytes = Vec::new();
        for (name, value) in [
            ("PGMNAME", &b"COTRTUPC"[..]),
            ("TRNNAME", &b"CTTU"[..]),
            ("INFOMSG", info),
            ("ERRMSG", error),
        ] {
            bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(value);
        }
        serde_json::json!({
            "mapset":"COTRTUP", "map":"CTRTUPA",
            "screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    #[test]
    fn complete_rows_reject_unchanged_count_wrong_tail_or_category() {
        let (types, categories) = expected_rows(Phase::Inserted);
        let mut tail = types.clone();
        tail.last_mut().unwrap()[1].push(b'!');
        assert!(!rows_match(Phase::Inserted, &tail, &categories));
        let mut category = categories.clone();
        category[17][2].push(b'!');
        assert!(!rows_match(Phase::Inserted, &types, &category));
    }

    #[test]
    fn complete_rows_reject_order_column_and_extra_row_drift() {
        let (types, categories) = expected_rows(Phase::Updated);
        let mut swapped = types.clone();
        swapped.swap(0, 1);
        assert!(!rows_match(Phase::Updated, &swapped, &categories));
        let mut column = types.clone();
        column[7].swap(0, 1);
        assert!(!rows_match(Phase::Updated, &column, &categories));
        let mut extra = types.clone();
        extra.push(types[7].clone());
        assert!(!rows_match(Phase::Updated, &extra, &categories));
    }

    #[test]
    fn phase_comparison_refuses_insert_update_delete_cross_credit() {
        let (types, categories) = expected_rows(Phase::Inserted);
        assert!(rows_match(Phase::Inserted, &types, &categories));
        assert!(!rows_match(Phase::Updated, &types, &categories));
        assert!(!rows_match(Phase::Deleted, &types, &categories));
    }

    #[test]
    fn exact_screens_reject_right_message_wrong_field_padding_or_map() {
        let good = screen(COMMITTED_INFO, EMPTY_ERROR);
        assert!(screen_matches(&good, Some(COMMITTED_INFO), EMPTY_ERROR).unwrap());
        let mut info = COMMITTED_INFO.to_vec();
        info.pop();
        let short = screen(&info, EMPTY_ERROR);
        assert!(!screen_matches(&short, Some(COMMITTED_INFO), EMPTY_ERROR).unwrap());
        let wrong_field = screen(EMPTY_ERROR, COMMITTED_INFO);
        assert!(!screen_matches(&wrong_field, Some(COMMITTED_INFO), EMPTY_ERROR).unwrap());
        let mut wrong_map = good;
        wrong_map["map"] = serde_json::json!("CTRTLIA");
        assert!(!screen_matches(&wrong_map, Some(COMMITTED_INFO), EMPTY_ERROR).unwrap());
    }

    #[test]
    fn missing_and_commit_screen_branches_cannot_share_credit() {
        let missing = screen(b"", MISSING_ERROR);
        assert!(screen_matches(&missing, None, MISSING_ERROR).unwrap());
        assert!(!screen_matches(&missing, Some(COMMITTED_INFO), EMPTY_ERROR).unwrap());
        assert!(require_committed(&missing, Phase::Seed).is_err());
    }

    fn replace_screen_field(response: &mut serde_json::Value, name: &str, value: Option<&[u8]>) {
        let mut fields = online_screen_fields(response).unwrap();
        match value {
            Some(value) => {
                fields.insert(name.into(), value.to_vec());
            }
            None => {
                fields.remove(name);
            }
        }
        let mut bytes = Vec::new();
        for (name, value) in fields {
            bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&value);
        }
        response["screen_base64"] =
            serde_json::json!(base64::engine::general_purpose::STANDARD.encode(bytes));
    }

    #[test]
    fn failure_diagnostics_identify_each_exact_field_without_acceptance() {
        let good = screen(b"", MISSING_ERROR);
        assert!(
            screen_diagnostic(&good, None, MISSING_ERROR)
                .unwrap()
                .is_empty()
        );
        for (name, expected) in [
            ("PGMNAME", &b"COTRTUPC"[..]),
            ("TRNNAME", &b"CTTU"[..]),
            ("ERRMSG", MISSING_ERROR),
        ] {
            let mut wrong = good.clone();
            let mut actual = expected.to_vec();
            actual[0] ^= 1;
            replace_screen_field(&mut wrong, name, Some(&actual));
            let problem = require_missing(&wrong).unwrap_err();
            assert_eq!(problem.code, "carddemo.db2.online_missing_screen_drift");
            assert!(
                problem
                    .detail
                    .contains(&format!("{name}: expected_length="))
            );
            assert!(problem.detail.contains("first_difference=0"));
            replace_screen_field(&mut wrong, name, None);
            assert!(
                require_missing(&wrong)
                    .unwrap_err()
                    .detail
                    .contains(&format!("{name}: missing or invalid type"))
            );
        }
        for name in ["mapset", "map"] {
            let mut wrong = good.clone();
            wrong[name] = serde_json::json!(false);
            assert!(
                require_missing(&wrong)
                    .unwrap_err()
                    .detail
                    .contains(&format!("{name}: missing or invalid type"))
            );
        }
    }

    #[test]
    fn failure_diagnostics_preserve_padding_and_do_not_publish_screen_values() {
        let mut wrong = screen(COMMITTED_INFO, EMPTY_ERROR);
        let mut short = COMMITTED_INFO.to_vec();
        short.pop();
        replace_screen_field(&mut wrong, "INFOMSG", Some(&short));
        let problem = require_committed(&wrong, Phase::Inserted).unwrap_err();
        assert_eq!(problem.code, "carddemo.db2.online_commit_screen_drift");
        assert!(
            problem
                .detail
                .contains("INFOMSG: expected_length=45; actual_length=44")
        );
        assert!(problem.detail.contains("first_difference=44"));
        assert!(!problem.detail.contains("Changes committed"));
        let mut wrong = screen(b"", MISSING_ERROR);
        replace_screen_field(&mut wrong, "ERRMSG", Some(b"sensitive-customer-canary"));
        let problem = require_missing(&wrong).unwrap_err();
        assert!(!problem.detail.contains("sensitive-customer-canary"));
        assert!(problem.detail.contains("actual_sha256="));
    }

    #[test]
    fn failure_diagnostics_do_not_bypass_malformed_symbolic_payload_refusal() {
        let malformed =
            serde_json::json!({"mapset":"COTRTUP", "map":"CTRTUPA", "screen_base64":"!"});
        assert_eq!(
            require_missing(&malformed).unwrap_err().code,
            "carddemo.online.response"
        );
        assert_eq!(
            screen_diagnostic(&malformed, None, MISSING_ERROR)
                .unwrap_err()
                .code,
            "carddemo.online.response"
        );
    }
}
