//! Actual user-security refusals compared with pinned map/program expectations.
use super::transaction_harness::RawRows;
use super::*;

const DUPLICATE_ASCII: &[u8; 78] =
    b"User ID already exist...                                                      ";
const DUPLICATE_CP037: &[u8; 78] = &[
    0xe4, 0xa2, 0x85, 0x99, 0x40, 0xc9, 0xc4, 0x40, 0x81, 0x93, 0x99, 0x85, 0x81, 0x84, 0xa8, 0x40,
    0x85, 0xa7, 0x89, 0xa2, 0xa3, 0x4b, 0x4b, 0x4b, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
];
const MISSING_ASCII: &[u8; 78] =
    b"User ID NOT found...                                                          ";
const MISSING_CP037: &[u8; 78] = &[
    0xe4, 0xa2, 0x85, 0x99, 0x40, 0xc9, 0xc4, 0x40, 0xd5, 0xd6, 0xe3, 0x40, 0x86, 0x96, 0xa4, 0x95,
    0x84, 0x4b, 0x4b, 0x4b, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
];

const USER_SECURITY_SOURCES: &[(&str, &str)] = &[
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/cbl/COADM01C.cbl",
        "4e49afda5f685f3ebeb95ed04a93cf88df93d39c66d8d7f9ba2b9a3e00a1f690",
    ),
    (
        "app/cbl/COUSR00C.cbl",
        "831433c6ec8306038c85cb86a8307f91dca48c26ef33a997f402187cbd9f4e04",
    ),
    (
        "app/cbl/COUSR01C.cbl",
        "aa131b1e3382dc6d101b42f1c97d4fb0c2fdd706819ed9f9a0187c82b30f3019",
    ),
    (
        "app/cbl/COUSR02C.cbl",
        "85d36699cbd3079320e689576ea8406d07ad78901f4f3a68a129b869359b46ca",
    ),
    (
        "app/cbl/COUSR03C.cbl",
        "bcd68f08c145b3b9b9c809a7082834e9519d78b6f363a0d1c5ff9d9576dcf888",
    ),
    (
        "app/bms/COADM01.bms",
        "44a3f209e7fa8445355ab7ba687b3ac6e32ef4dfec225e04f4eb2bd57e39386a",
    ),
    (
        "app/bms/COUSR00.bms",
        "82a453530e4457e45f8af028b68307f7049e24fa13fedb8644007f68a9738f88",
    ),
    (
        "app/bms/COUSR01.bms",
        "f79efda027a728fad77ca163b1a85851e099df5d3ce6876309274dbb2b67dff9",
    ),
    (
        "app/bms/COUSR02.bms",
        "324ff2e74f580d061a15963d394c9883eedf93dd7de95a04926f78bb78aecff1",
    ),
    (
        "app/bms/COUSR03.bms",
        "95f557d76cb482753039cc60f62cbac4fe09c4c02f9057658ee1ee70429c6fc9",
    ),
    (
        "app/cpy/CSUSR01Y.cpy",
        "37c34094dc4124bc6255e0b06a0387442f4f0dd9d51dd6623b559d370aa0d2cb",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS",
        "8608d4b8a03f7587c1493282f9337ab24fdf527d0aadbdb85dc3713dccdc6fc8",
    ),
];

pub(super) fn verify_user_security_sources() -> Result<(), CorpusProblem> {
    let corpus = env::var_os(CORPUS_ENV).map(PathBuf::from).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?;
    for (relative, expected) in USER_SECURITY_SOURCES {
        let bytes = read_corpus_file(&corpus, &corpus.join(relative))?;
        if format!("{:x}", Sha256::digest(&bytes)) != *expected {
            return Err(CorpusProblem::new(
                "carddemo.online.user_security_source_drift",
                *relative,
            ));
        }
    }
    Ok(())
}

pub(super) fn verify_user_security_seed(server: &ProductServer) -> Result<(), CorpusProblem> {
    verify_user_security_sources()?;
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("verified corpus environment"));
    let bytes = read_corpus_file(
        &corpus,
        &corpus.join("app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS"),
    )?;
    // CSUSR01Y defines 80-byte rows with the original eight-byte key. The pinned
    // first-party seed is exactly 800 bytes; never derive expected rows from a
    // provider read or a decoded/re-encoded product record.
    let seed_pin = USER_SECURITY_SOURCES
        .iter()
        .find(|(path, _)| path.ends_with("USRSEC.PS"))
        .expect("frozen USRSEC seed pin")
        .1;
    if bytes.len() != 800 || format!("{:x}", Sha256::digest(&bytes)) != seed_pin {
        return Err(CorpusProblem::new(
            "carddemo.online.user_security_source_drift",
            "USRSEC seed bytes changed",
        ));
    }
    let mut expected = bytes
        .as_chunks::<80>()
        .0
        .iter()
        .map(|row| row.to_vec())
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| left[..8].cmp(&right[..8]));
    let expected_keys = expected
        .iter()
        .map(|row| row[..8].to_vec())
        .collect::<Vec<_>>();
    let snapshot = transaction_harness::snapshot(server)?;
    if snapshot[2].0 != expected || snapshot[2].1 != expected_keys {
        return Err(CorpusProblem::new(
            "carddemo.online.user_security_seed_drift",
            "USRSEC raw rows/keys differ from the pinned first-party seed",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum UserRefusal {
    Duplicate,
    Missing,
}

pub(super) fn user_refusal_matches(
    refusal: UserRefusal,
    response: &serde_json::Value,
    trace: &[mainframe_env_cics::CicsTraceEntry],
    before: &[RawRows],
    after: &[RawRows],
) -> Result<bool, CorpusProblem> {
    let (mapset, ascii, ebcdic, operation, condition, primary, secondary) = match refusal {
        UserRefusal::Duplicate => (
            "COUSR01",
            DUPLICATE_ASCII,
            DUPLICATE_CP037,
            CicsOperation::Write,
            "DUPREC",
            14,
            0,
        ),
        UserRefusal::Missing => (
            "COUSR02",
            MISSING_ASCII,
            MISSING_CP037,
            CicsOperation::Read,
            "NOTFND",
            13,
            80,
        ),
    };
    // Frozen independently from first-party message literals/BMS LENGTH=78.
    // The current opaque symbolic-map screen contract admits literal ASCII or
    // Cp037 bytes. Each alternative includes every trailing padding byte.
    let fields = online_screen_fields(response)?;
    let effects = trace
        .iter()
        .filter(|entry| entry.operation == operation)
        .collect::<Vec<_>>();
    let map = match refusal {
        UserRefusal::Duplicate => "COUSR1A",
        UserRefusal::Missing => "COUSR2A",
    };
    Ok(response["mapset"] == mapset
        && response["map"] == map
        && fields
            .get("ERRMSG")
            .is_some_and(|actual| actual.as_slice() == ascii || actual.as_slice() == ebcdic)
        && before.len() == 5
        && after == before
        && effects.len() == 1
        && effects[0].outcome == condition
        && effects[0].response == primary
        && effects[0].response2 == secondary
        && !trace.iter().any(|entry| {
            matches!(
                entry.operation,
                CicsOperation::Write | CicsOperation::Rewrite | CicsOperation::Delete
            ) && entry.outcome == "NORMAL"
        }))
}

pub(super) fn observe_user_conditions(
    observations: &mut RouteObservations,
    duplicate: bool,
    missing: bool,
) -> Result<(), CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J08",
        "duplicate/not-found conditions",
        !duplicate || !missing,
        || Ok(CorpusProblem::new(
            "carddemo.online.user_condition_drift",
            "duplicate/not-found routes did not return exact pinned messages and actual conditions without changing raw rows, keys or versions",
        )),
    )
}

pub(super) fn exact_forbidden(status: StatusCode, body: &[u8]) -> bool {
    status == StatusCode::FORBIDDEN
        && serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|value| {
            value
                == serde_json::json!({
                    "category":"not_authorized",
                    "message":"host service failed: Unauthorized",
                    "status":403
                })
        })
}

pub(super) async fn remaining_admin_denials(
    server: &ProductServer,
    app: &axum::Router,
    regular_authorization: &str,
    first: &(StatusCode, Vec<u8>),
    before: &[RawRows],
) -> Result<bool, CorpusProblem> {
    let mut denied = exact_forbidden(first.0, &first.1);
    // CU01 is the original checked request. These are the four other actual
    // administrative transactions declared by the pinned CARDDEMO.CSD.
    for transaction in ["CA00", "CU00", "CU02", "CU03"] {
        let response = terminal_http(
            app,
            Method::POST,
            "/mainframe-env/cics/v1/sessions",
            BTreeMap::from([
                ("authorization".into(), regular_authorization.into()),
                ("x-csrf-zosmf-header".into(), "true".into()),
            ]),
            serde_json::to_vec(&serde_json::json!({"transaction":transaction})).map_err(
                |error| CorpusProblem::new("carddemo.online.response", error.to_string()),
            )?,
        )
        .await?;
        denied &= exact_forbidden(response.0, &response.1);
    }
    Ok(denied && before.len() == 5 && transaction_harness::snapshot(server)? == before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(mapset: &str, message: &str) -> serde_json::Value {
        let mut value = message.as_bytes().to_vec();
        value.resize(78, b' ');
        let mut field = 6u32.to_be_bytes().to_vec();
        field.extend_from_slice(b"ERRMSG");
        field.extend_from_slice(&78u32.to_be_bytes());
        field.extend_from_slice(&value);
        let map = match mapset {
            "COUSR01" => "COUSR1A",
            "COUSR02" => "COUSR2A",
            _ => "invalid",
        };
        serde_json::json!({"mapset":mapset,"map":map,"screen_base64":base64::engine::general_purpose::STANDARD.encode(field)})
    }

    fn trace(
        operation: CicsOperation,
        outcome: &str,
        response: i32,
        response2: i32,
    ) -> Vec<mainframe_env_cics::CicsTraceEntry> {
        vec![mainframe_env_cics::CicsTraceEntry {
            operation,
            outcome: outcome.into(),
            response,
            response2,
            payload_bytes: 0,
        }]
    }

    fn snapshots() -> Vec<RawRows> {
        // Synthetic comparison controls earn no actual-route acceptance credit.
        vec![(vec![vec![0xc1; 80]], vec![vec![0xc1; 8]], 7); 5]
    }

    #[test]
    fn user_refusal_rejects_same_count_record_key_and_version_changes() {
        let before = snapshots();
        let actual = response("COUSR01", "User ID already exist...");
        let effects = trace(CicsOperation::Write, "DUPREC", 14, 0);
        assert!(
            user_refusal_matches(UserRefusal::Duplicate, &actual, &effects, &before, &before)
                .unwrap()
        );
        for component in 0..3 {
            let mut after = before.clone();
            match component {
                0 => after[2].0[0][40] ^= 1,
                1 => after[2].1[0][0] ^= 1,
                _ => after[2].2 += 1,
            }
            assert!(
                !user_refusal_matches(UserRefusal::Duplicate, &actual, &effects, &before, &after)
                    .unwrap()
            );
        }
    }

    #[test]
    fn user_refusal_rejects_forged_map_message_or_condition() {
        let before = snapshots();
        let actual = response("COUSR02", "User ID NOT found...");
        let effects = trace(CicsOperation::Read, "NOTFND", 13, 80);
        assert!(
            user_refusal_matches(UserRefusal::Missing, &actual, &effects, &before, &before)
                .unwrap()
        );
        for wrong_map in [serde_json::Value::Null, serde_json::json!("COUSR1A")] {
            let mut forged = actual.clone();
            forged["map"] = wrong_map;
            assert!(
                !user_refusal_matches(UserRefusal::Missing, &forged, &effects, &before, &before)
                    .unwrap()
            );
        }
        assert!(
            !user_refusal_matches(
                UserRefusal::Missing,
                &response("COUSR01", "User ID NOT found..."),
                &effects,
                &before,
                &before
            )
            .unwrap()
        );
        assert!(
            !user_refusal_matches(
                UserRefusal::Missing,
                &response("COUSR02", "Unable to lookup User..."),
                &effects,
                &before,
                &before
            )
            .unwrap()
        );
        for bad in [
            Vec::new(),
            trace(CicsOperation::Write, "NOTFND", 13, 80),
            trace(CicsOperation::Read, "NORMAL", 0, 0),
            trace(CicsOperation::Read, "NOTFND", 13, 0),
        ] {
            assert!(
                !user_refusal_matches(UserRefusal::Missing, &actual, &bad, &before, &before)
                    .unwrap()
            );
        }
    }

    #[test]
    fn combined_user_observation_requires_both_executed_comparisons() {
        for (duplicate, missing) in [(false, false), (true, false), (false, true)] {
            let mut observed = RouteObservations::default();
            assert!(observe_user_conditions(&mut observed, duplicate, missing).is_err());
            assert!(observed.journeys().is_empty());
        }
    }

    #[test]
    fn frozen_error_messages_require_the_entire_padding_in_both_encodings() {
        let before = snapshots();
        let effects = trace(CicsOperation::Read, "NOTFND", 13, 80);
        for bytes in [MISSING_ASCII, MISSING_CP037] {
            let mut field = 6u32.to_be_bytes().to_vec();
            field.extend_from_slice(b"ERRMSG");
            field.extend_from_slice(&78u32.to_be_bytes());
            field.extend_from_slice(bytes);
            let actual = serde_json::json!({"mapset":"COUSR02","map":"COUSR2A","screen_base64":base64::engine::general_purpose::STANDARD.encode(&field)});
            assert!(
                user_refusal_matches(UserRefusal::Missing, &actual, &effects, &before, &before)
                    .unwrap()
            );
            *field.last_mut().unwrap() ^= 1;
            let drifted = serde_json::json!({"mapset":"COUSR02","map":"COUSR2A","screen_base64":base64::engine::general_purpose::STANDARD.encode(field)});
            assert!(
                !user_refusal_matches(UserRefusal::Missing, &drifted, &effects, &before, &before)
                    .unwrap()
            );
        }
    }

    #[test]
    fn forbidden_body_cannot_hide_an_owner_payload() {
        let exact = serde_json::json!({"category":"not_authorized","message":"host service failed: Unauthorized","status":403});
        let bytes = serde_json::to_vec(&exact).unwrap();
        assert!(exact_forbidden(StatusCode::FORBIDDEN, &bytes));
        assert!(!exact_forbidden(StatusCode::OK, &bytes));
        let mut leaked = exact;
        leaked["jobid"] = "JOB00001".into();
        assert!(!exact_forbidden(
            StatusCode::FORBIDDEN,
            &serde_json::to_vec(&leaked).unwrap()
        ));
    }
}
