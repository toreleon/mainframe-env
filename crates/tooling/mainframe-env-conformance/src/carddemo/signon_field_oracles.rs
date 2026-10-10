//! Source-frozen complete COSGN00 fields and bounded actual typed-AID controls.
// Depends on the separately owned message routes/date offer; it adds no second
// wrong-password path. Source: CardDemo 59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e.
// Source/BMS/CPY own every named field and padding byte. APPLID ME01 / SYSID S001
// are the existing local fixture; no IBM identity parity is claimed. The current
// opaque symbolic screen permits independently frozen ASCII or Cp037 fields.
// PASSWD is exactly empty under the existing secret-display contract. Launch
// USERID is eight source LOW-VALUES; resumed USERID is the offered input padded
// to its source width. No expected value comes from product output or a codec.
// Typed PUT receipts and fresh RECEIVE tuples are useful local seams. They do
// not expose received field byte contents/MDT, passed storage or RETURN native
// retirement. Therefore this module never emits "AID and MDT input" or
// "RETURN TRANSID suspension" and changes no global acceptance denominator.
use super::*;
use mainframe_env_cics::CicsTraceEntry;

const OFFERED_DATE: &str = "2022070600000000+0000";
const SOURCE_PINS: &[(&str, &str)] = &[
    (
        "app/cbl/COSGN00C.cbl",
        "4f901ae6b113eebae60ae1d94b30f8cb9906d031a4419770bf56980bdf97e377",
    ),
    (
        "app/bms/COSGN00.bms",
        "6747ba57e45fed7e52af728a1f8688ed8fab90b93e1234ab0a0bcae32fa39056",
    ),
    (
        "app/cpy-bms/COSGN00.CPY",
        "e2a2f8f6e76e1613cbd8f0baaca87d571a49a7bc219d16a732e4e4ecd7485407",
    ),
    (
        "app/cpy/COTTL01Y.cpy",
        "5032bfa9428354bb4a62e8c82ba1efa7dcf6b80f27544ad99a81c8a8df8ed41b",
    ),
    (
        "app/cpy/CSDAT01Y.cpy",
        "61dd7b01d974c13cd47f71774c613dd67996418a22fae730d09b246c0b710ad7",
    ),
    (
        "app/cpy/CSMSG01Y.cpy",
        "ef8ce44572885d63e46a02516f992cabbbc57ea94235000fd376325750e26eda",
    ),
    (
        "app/cpy/COCOM01Y.cpy",
        "77097557ee4c365f62138328d3e280ce7aba5f81d418c6f16434f0fbb2aafa6c",
    ),
];

const HEADERS: [(&str, &[u8], &[u8]); 8] = [
    ("TRNNAME", b"CC00", b"\xc3\xc3\xf0\xf0"),
    ("TITLE01", b"      AWS Mainframe Modernization       ", b"\x40\x40\x40\x40\x40\x40\xc1\xe6\xe2\x40\xd4\x81\x89\x95\x86\x99\x81\x94\x85\x40\xd4\x96\x84\x85\x99\x95\x89\xa9\x81\xa3\x89\x96\x95\x40\x40\x40\x40\x40\x40\x40"),
    ("CURDATE", b"07/06/22", b"\xf0\xf7\x61\xf0\xf6\x61\xf2\xf2"),
    ("PGMNAME", b"COSGN00C", b"\xc3\xd6\xe2\xc7\xd5\xf0\xf0\xc3"),
    ("TITLE02", b"              CardDemo                  ", b"\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xc3\x81\x99\x84\xc4\x85\x94\x96\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40"),
    ("CURTIME", b"00:00:00 ", b"\xf0\xf0\x7a\xf0\xf0\x7a\xf0\xf0\x40"),
    ("APPLID", b"ME01    ", b"\xd4\xc5\xf0\xf1\x40\x40\x40\x40"),
    ("SYSID", b"S001    ", b"\xe2\xf0\xf0\xf1\x40\x40\x40\x40"),
];
const BLANK_MESSAGE: (&[u8], &[u8]) = (b"                                                                              ", b"\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40");
const NOT_FOUND_MESSAGE: (&[u8], &[u8]) = (b"User not found. Try again ...                                                 ", b"\xe4\xa2\x85\x99\x40\x95\x96\xa3\x40\x86\x96\xa4\x95\x84\x4b\x40\xe3\x99\xa8\x40\x81\x87\x81\x89\x95\x40\x4b\x4b\x4b\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40");
const WRONG_PASSWORD_MESSAGE: (&[u8], &[u8]) = (b"Wrong Password. Try again ...                                                 ", b"\xe6\x99\x96\x95\x87\x40\xd7\x81\xa2\xa2\xa6\x96\x99\x84\x4b\x40\xe3\x99\xa8\x40\x81\x87\x81\x89\x95\x40\x4b\x4b\x4b\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40");
const INVALID_KEY_MESSAGE: (&[u8], &[u8]) = (b"Invalid key pressed. Please see below...                                      ", b"\xc9\x95\xa5\x81\x93\x89\x84\x40\x92\x85\xa8\x40\x97\x99\x85\xa2\xa2\x85\x84\x4b\x40\xd7\x93\x85\x81\xa2\x85\x40\xa2\x85\x85\x40\x82\x85\x93\x96\xa6\x4b\x4b\x4b\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40");
const EMPTY_USER_MESSAGE: (&[u8], &[u8]) = (b"Please enter User ID ...                                                      ", b"\xd7\x93\x85\x81\xa2\x85\x40\x85\x95\xa3\x85\x99\x40\xe4\xa2\x85\x99\x40\xc9\xc4\x40\x4b\x4b\x4b\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40");
const NOUSER: (&[u8], &[u8]) = (b"NOUSER  ", b"\xd5\xd6\xe4\xe2\xc5\xd9\x40\x40");
const USER0001: (&[u8], &[u8]) = (b"USER0001", b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf1");
const EMPTY_USER: (&[u8], &[u8]) = (b"        ", b"\x40\x40\x40\x40\x40\x40\x40\x40");

pub(super) struct LaunchFieldsProof {
    _private: (),
}
pub(super) struct NotFoundFieldsProof {
    _private: (),
}

#[derive(Clone, Copy)]
enum Screen {
    Launch,
    NotFound,
    WrongPassword,
    EmptyUser,
}

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.signon_field_drift", detail)
}

pub(super) fn require_clock_input(offered: Option<&str>) -> Result<(), CorpusProblem> {
    if offered != Some(OFFERED_DATE) {
        return Err(drift("selected sign-on current-date input differs"));
    }
    Ok(())
}

fn verify_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (relative, expected) in SOURCE_PINS {
        let bytes = read_corpus_file(corpus, &corpus.join(relative))?;
        if format!("{:x}", Sha256::digest(&bytes)) != *expected {
            return Err(drift(format!(
                "pinned sign-on field source differs: {relative}"
            )));
        }
    }
    Ok(())
}

fn field_matches(fields: &BTreeMap<String, Vec<u8>>, name: &str, expected: (&[u8], &[u8])) -> bool {
    fields
        .get(name)
        .is_some_and(|value| value.as_slice() == expected.0 || value.as_slice() == expected.1)
}

fn headers_match(fields: &BTreeMap<String, Vec<u8>>) -> bool {
    HEADERS
        .iter()
        .all(|(name, ascii, cp037)| field_matches(fields, name, (ascii, cp037)))
}

fn fields_match(fields: &BTreeMap<String, Vec<u8>>, screen: Screen) -> bool {
    let (user, message) = match screen {
        Screen::Launch => (
            (
                b"\0\0\0\0\0\0\0\0".as_slice(),
                b"\0\0\0\0\0\0\0\0".as_slice(),
            ),
            BLANK_MESSAGE,
        ),
        Screen::NotFound => (NOUSER, NOT_FOUND_MESSAGE),
        Screen::WrongPassword => (USER0001, WRONG_PASSWORD_MESSAGE),
        Screen::EmptyUser => (EMPTY_USER, EMPTY_USER_MESSAGE),
    };
    fields.len() == 11
        && headers_match(fields)
        && field_matches(fields, "USERID", user)
        && fields.get("PASSWD").is_some_and(Vec::is_empty)
        && field_matches(fields, "ERRMSG", message)
}

fn screen_matches(terminal: &serde_json::Value, screen: Screen) -> Result<bool, CorpusProblem> {
    Ok(terminal["mapset"] == "COSGN00"
        && terminal["map"] == "COSGN0A"
        && fields_match(&online_screen_fields(terminal)?, screen))
}

pub(super) fn compare_launch(
    corpus: &Path,
    terminal: &serde_json::Value,
) -> Result<LaunchFieldsProof, CorpusProblem> {
    verify_sources(corpus)?;
    if !screen_matches(terminal, Screen::Launch)? {
        return Err(drift("all eleven launch fields differ"));
    }
    Ok(LaunchFieldsProof { _private: () })
}

pub(super) fn compare_not_found(
    terminal: &serde_json::Value,
    _launch: LaunchFieldsProof,
) -> Result<NotFoundFieldsProof, CorpusProblem> {
    if !screen_matches(terminal, Screen::NotFound)? {
        return Err(drift("all eleven NOTFND-resume fields differ"));
    }
    Ok(NotFoundFieldsProof { _private: () })
}

pub(super) fn compare_wrong_password(
    terminal: &serde_json::Value,
    _not_found: NotFoundFieldsProof,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J02",
        "COSGN00 screen fields",
        !screen_matches(terminal, Screen::WrongPassword)?,
        || Ok(drift("all eleven wrong-password-resume fields differ")),
    )
}

fn public_signon_state(
    terminal: &serde_json::Value,
    session: &str,
    aid: u8,
    suspended: bool,
) -> bool {
    terminal["schema_version"] == "mainframe-env.cics-terminal@1"
        && terminal["session"] == session
        && terminal["principal"] == "WEBUSER"
        && terminal["transaction"] == "CC00"
        && terminal["rows"] == 24
        && terminal["columns"] == 80
        && terminal["aid"].as_u64() == Some(u64::from(aid))
        && terminal["suspended"] == suspended
        && terminal["connected"] == true
        && terminal["mapset"] == "COSGN00"
        && terminal["map"] == "COSGN0A"
        && terminal["version"]
            .as_u64()
            .is_some_and(|version| version != 0)
        && terminal["run_unit"]
            .as_str()
            .is_some_and(|run| !run.is_empty())
}

fn input_receipt_matches(
    before: &serde_json::Value,
    receipt: &serde_json::Value,
    session: &str,
    aid: u8,
) -> bool {
    public_signon_state(receipt, session, aid, false)
        && before["version"]
            .as_u64()
            .and_then(|version| version.checked_add(1))
            == receipt["version"].as_u64()
        && receipt["screen_base64"] == before["screen_base64"]
        && receipt["run_unit"] == before["run_unit"]
}

pub(super) fn require_input_receipt(
    before: &serde_json::Value,
    body: &[u8],
    session: &str,
    aid: u8,
) -> Result<(), CorpusProblem> {
    let receipt: serde_json::Value =
        serde_json::from_slice(body).map_err(|error| drift(error.to_string()))?;
    if !input_receipt_matches(before, &receipt, session, aid) {
        return Err(drift(
            "actual typed sign-on input receipt state, AID or revision differs",
        ));
    }
    Ok(())
}

fn receive_matches(trace: &[CicsTraceEntry], count: usize, payload: usize) -> bool {
    let receives = trace
        .iter()
        .filter(|entry| entry.operation == CicsOperation::ReceiveMap)
        .collect::<Vec<_>>();
    receives.len() == count
        && receives.iter().all(|entry| {
            entry.outcome == "NORMAL"
                && entry.response == 0
                && entry.response2 == 0
                && entry.payload_bytes == payload
        })
}

pub(super) fn require_not_found_typed_resume(
    terminal: &serde_json::Value,
    session: &str,
    trace: &[CicsTraceEntry],
) -> Result<(), CorpusProblem> {
    // Eleven named FSET fields contribute 71 name + 88 length-prefix bytes.
    // Header/message source widths total203; actual offer NOUSER6/BADPASS7
    // contributes13. The existing typed FSET carry contract therefore exposes
    // a375-byte RECEIVE tuple. Exact received password bytes are not exposed.
    // Supplied ENTER completes RECEIVE and the source's ordinary RETURN stage.
    // Genuine pending RECEIVE suspension requires a separate absent-input offer;
    // this typed-input comparison earns no RETURN TRANSID suspension observation.
    if !public_signon_state(terminal, session, 0x7d, false) || !receive_matches(trace, 1, 375) {
        return Err(drift("actual typed ENTER/RECEIVE sign-on seam differs"));
    }
    Ok(())
}

fn no_file_effects(trace: &[CicsTraceEntry]) -> bool {
    !trace.iter().any(|entry| {
        matches!(
            entry.operation,
            CicsOperation::Read
                | CicsOperation::Write
                | CicsOperation::Rewrite
                | CicsOperation::Delete
                | CicsOperation::StartBrowse
                | CicsOperation::ReadNext
                | CicsOperation::ReadPrev
                | CicsOperation::EndBrowse
                | CicsOperation::ResetBrowse
                | CicsOperation::Unlock
        )
    })
}

pub(super) async fn exercise_typed_controls(
    server: &ProductServer,
    app: &axum::Router,
    corpus: &Path,
) -> Result<(), CorpusProblem> {
    verify_sources(corpus)?;
    let authentication = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("WEBUSER:transport-password")
    );
    let launch = terminal_http(
        app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), authentication.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CC00"}"#.to_vec(),
    )
    .await?;
    require_terminal_status(
        launch.0,
        StatusCode::CREATED,
        "typed sign-on control launch",
    )?;
    let launched: serde_json::Value =
        serde_json::from_slice(&launch.1).map_err(|error| drift(error.to_string()))?;
    let session = launched["session"]
        .as_str()
        .ok_or_else(|| drift("typed control session missing"))?;
    let csrf = launched["csrf_token"]
        .as_str()
        .ok_or_else(|| drift("typed control CSRF missing"))?;
    let before = &launched["terminal"];
    let _launch = compare_launch(corpus, before)?;
    // Initial EIBCALEN=0 sends the sign-on map then performs ordinary RETURN.
    // It never enters RECEIVE; genuine absent-input suspension is a separate stage.
    if !public_signon_state(before, session, 0, false) {
        return Err(drift("typed control launch public state differs"));
    }
    let headers = BTreeMap::from([
        ("authorization".into(), authentication),
        ("x-csrf-zosmf-header".into(), "true".into()),
        ("x-csrf-token".into(), csrf.into()),
    ]);
    let at = server
        .online_trace(session)
        .map_err(terminal_problem)?
        .len();
    let rejected = terminal_http(
        app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        headers.clone(),
        br#"{"aid":0,"fields":{}}"#.to_vec(),
    )
    .await?;
    require_terminal_status(rejected.0, StatusCode::BAD_REQUEST, "unsupported typed AID")?;
    let stored = terminal_http(
        app,
        Method::GET,
        &format!("/mainframe-env/cics/v1/sessions/{session}"),
        headers.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(stored.0, StatusCode::OK, "typed AID refusal read-back")?;
    let stored: serde_json::Value =
        serde_json::from_slice(&stored.1).map_err(|error| drift(error.to_string()))?;
    if &stored != before || !base_message_oracles::trace_since(server, session, at)?.is_empty() {
        return Err(drift(
            "unsupported typed AID changed public state or executed a CICS command",
        ));
    }
    let aid = 0xf1;
    let accepted = terminal_http(
        app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        headers.clone(),
        br#"{"aid":241,"fields":{}}"#.to_vec(),
    )
    .await?;
    require_terminal_status(accepted.0, StatusCode::OK, "non-ENTER typed sign-on input")?;
    require_input_receipt(before, &accepted.1, session, aid)?;
    let resumed = terminal_http(
        app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{session}/resume"),
        headers.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(resumed.0, StatusCode::OK, "non-ENTER typed sign-on resume")?;
    let resumed: serde_json::Value =
        serde_json::from_slice(&resumed.1).map_err(|error| drift(error.to_string()))?;
    let fresh = base_message_oracles::trace_since(server, session, at)?;
    let fields = online_screen_fields(&resumed)?;
    // Only the source-owned header/message/secret fields are promised on this
    // non-RECEIVE branch; this partial vector earns no full screen row itself.
    if !public_signon_state(&resumed, session, aid, false)
        || !headers_match(&fields)
        || !field_matches(&fields, "ERRMSG", INVALID_KEY_MESSAGE)
        || !fields.get("PASSWD").is_some_and(Vec::is_empty)
        || !receive_matches(&fresh, 0, 0)
        || !no_file_effects(&fresh)
    {
        return Err(drift(
            "actual non-ENTER branch performed RECEIVE/read or rendered wrong source fields",
        ));
    }
    let at = server
        .online_trace(session)
        .map_err(terminal_problem)?
        .len();
    let empty = terminal_http(
        app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        headers.clone(),
        br#"{"aid":125,"fields":{"USERID":"","PASSWD":""}}"#.to_vec(),
    )
    .await?;
    require_terminal_status(empty.0, StatusCode::OK, "empty typed sign-on input")?;
    require_input_receipt(&resumed, &empty.1, session, 0x7d)?;
    let empty = terminal_http(
        app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{session}/resume"),
        headers,
        Vec::new(),
    )
    .await?;
    require_terminal_status(empty.0, StatusCode::OK, "empty typed sign-on resume")?;
    let empty: serde_json::Value =
        serde_json::from_slice(&empty.1).map_err(|error| drift(error.to_string()))?;
    let fresh = base_message_oracles::trace_since(server, session, at)?;
    // Source fixed header/message widths203 + name/prefix159 + empty offers0.
    if !public_signon_state(&empty, session, 0x7d, false)
        || !screen_matches(&empty, Screen::EmptyUser)?
        || !receive_matches(&fresh, 1, 362)
        || !no_file_effects(&fresh)
    {
        return Err(drift(
            "actual empty ENTER branch performed a read or failed the fixed typed field contract",
        ));
    }
    // No journey identity: raw MDT flags/storage/RETURN retirement stay unearned.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(screen: Screen, cp037: bool) -> BTreeMap<String, Vec<u8>> {
        let mut fields = HEADERS
            .iter()
            .map(|(name, ascii, ebcdic)| {
                (
                    (*name).into(),
                    if cp037 { *ebcdic } else { *ascii }.to_vec(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let (user, message) = match screen {
            Screen::Launch => (
                (
                    b"\0\0\0\0\0\0\0\0".as_slice(),
                    b"\0\0\0\0\0\0\0\0".as_slice(),
                ),
                BLANK_MESSAGE,
            ),
            Screen::NotFound => (NOUSER, NOT_FOUND_MESSAGE),
            Screen::WrongPassword => (USER0001, WRONG_PASSWORD_MESSAGE),
            Screen::EmptyUser => (EMPTY_USER, EMPTY_USER_MESSAGE),
        };
        fields.insert(
            "USERID".into(),
            if cp037 { user.1 } else { user.0 }.to_vec(),
        );
        fields.insert("PASSWD".into(), Vec::new());
        fields.insert(
            "ERRMSG".into(),
            if cp037 { message.1 } else { message.0 }.to_vec(),
        );
        fields
    }

    fn terminal(
        fields: &BTreeMap<String, Vec<u8>>,
        aid: u8,
        suspended: bool,
        version: u64,
    ) -> serde_json::Value {
        let mut bytes = Vec::new();
        for (name, value) in fields {
            bytes.extend((name.len() as u32).to_be_bytes());
            bytes.extend(name.as_bytes());
            bytes.extend((value.len() as u32).to_be_bytes());
            bytes.extend(value);
        }
        serde_json::json!({"schema_version":"mainframe-env.cics-terminal@1", "session":"FIXTURE", "principal":"WEBUSER", "transaction":"CC00", "run_unit":"RUN", "rows":24, "columns":80, "aid":aid, "screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes), "mapset":"COSGN00", "map":"COSGN0A", "suspended":suspended, "connected":true, "version":version})
    }

    #[test]
    fn signon_vector_rejects_every_changed_missing_short_long_and_extra_field() {
        for screen in [
            Screen::Launch,
            Screen::NotFound,
            Screen::WrongPassword,
            Screen::EmptyUser,
        ] {
            for cp037 in [false, true] {
                let good = fields(screen, cp037);
                assert!(fields_match(&good, screen));
                for (name, value) in &good {
                    for byte in 0..value.len() {
                        let mut bad = good.clone();
                        bad.get_mut(name).unwrap()[byte] ^= 1;
                        assert!(!fields_match(&bad, screen), "{name} byte={byte}");
                    }
                    let mut bad = good.clone();
                    bad.remove(name);
                    assert!(!fields_match(&bad, screen));
                    let mut bad = good.clone();
                    bad.get_mut(name).unwrap().push(0);
                    assert!(!fields_match(&bad, screen));
                    if !value.is_empty() {
                        let mut bad = good.clone();
                        bad.get_mut(name).unwrap().pop();
                        assert!(!fields_match(&bad, screen));
                    }
                }
                let mut bad = good.clone();
                bad.insert("EXTRA".into(), Vec::new());
                assert!(!fields_match(&bad, screen));
            }
        }
    }

    #[test]
    fn signon_vector_rejects_wrong_identity_clock_and_concealed_secret() {
        let good = terminal(&fields(Screen::WrongPassword, false), 0x7d, true, 5);
        assert!(screen_matches(&good, Screen::WrongPassword).unwrap());
        for (name, value) in [("mapset", "COSGN01"), ("map", "COMEN1A")] {
            let mut bad = good.clone();
            bad[name] = value.into();
            assert!(!screen_matches(&bad, Screen::WrongPassword).unwrap());
        }
        for secret in [
            b"BADPASS".as_slice(),
            b"\xc2\xc1\xc4\xd7\xc1\xe2\xe2".as_slice(),
        ] {
            let mut bad = fields(Screen::WrongPassword, false);
            bad.insert("PASSWD".into(), secret.to_vec());
            assert!(
                !screen_matches(&terminal(&bad, 0x7d, true, 5), Screen::WrongPassword).unwrap()
            );
        }
        assert!(require_clock_input(Some(OFFERED_DATE)).is_ok());
        for bad in [
            None,
            Some("2022070600000001+0000"),
            Some("2022080600000000+0000"),
        ] {
            assert!(require_clock_input(bad).is_err());
        }
    }

    #[test]
    fn typed_launch_requires_completed_send_return_stage_and_original_public_identity() {
        let completed = terminal(&fields(Screen::Launch, false), 0, false, 4);
        assert!(public_signon_state(&completed, "FIXTURE", 0, false));
        assert!(screen_matches(&completed, Screen::Launch).unwrap());
        for (name, value) in [
            ("suspended", serde_json::json!(true)),
            ("schema_version", serde_json::json!("unbound@1")),
            ("session", serde_json::json!("OTHER")),
            ("principal", serde_json::json!("WEBADM")),
            ("transaction", serde_json::json!("CC01")),
            ("rows", serde_json::json!(25)),
            ("columns", serde_json::json!(79)),
            ("aid", serde_json::json!(0x7d)),
            ("connected", serde_json::json!(false)),
            ("mapset", serde_json::json!("COSGN01")),
            ("map", serde_json::json!("COMEN1A")),
            ("version", serde_json::json!(0)),
            ("run_unit", serde_json::json!("")),
        ] {
            let mut bad = completed.clone();
            bad[name] = value;
            assert!(!public_signon_state(&bad, "FIXTURE", 0, false), "{name}");
        }
        // A separate actual absent-input RECEIVE can satisfy the true-state
        // predicate; that state is refused for this source's launch stage.
        let mut pending_receive = completed.clone();
        pending_receive["suspended"] = true.into();
        assert!(public_signon_state(&pending_receive, "FIXTURE", 0, true));
        assert!(!public_signon_state(&pending_receive, "FIXTURE", 0, false));
    }

    #[test]
    fn typed_receipt_requires_actual_aid_identity_state_revision_and_unchanged_screen() {
        let before = terminal(&fields(Screen::Launch, false), 0, true, 4);
        let good = terminal(&fields(Screen::Launch, false), 0x7d, false, 5);
        assert!(input_receipt_matches(&before, &good, "FIXTURE", 0x7d));
        for (name, value) in [
            ("aid", serde_json::json!(0xf1)),
            ("session", serde_json::json!("OTHER")),
            ("principal", serde_json::json!("WEBADM")),
            ("transaction", serde_json::json!("CC01")),
            ("suspended", serde_json::json!(true)),
            ("connected", serde_json::json!(false)),
            ("version", serde_json::json!(4)),
            ("version", serde_json::json!(6)),
            ("rows", serde_json::json!(25)),
            ("columns", serde_json::json!(79)),
            ("screen_base64", serde_json::json!("")),
            ("run_unit", serde_json::json!("OTHER")),
        ] {
            let mut bad = good.clone();
            bad[name] = value;
            assert!(
                !input_receipt_matches(&before, &bad, "FIXTURE", 0x7d),
                "{name}"
            );
        }
    }

    #[test]
    fn typed_receive_refuses_missing_extra_condition_and_payload_drift() {
        let good = vec![CicsTraceEntry {
            operation: CicsOperation::ReceiveMap,
            outcome: "NORMAL".into(),
            response: 0,
            response2: 0,
            payload_bytes: 375,
        }];
        assert!(receive_matches(&good, 1, 375));
        let completed = terminal(&fields(Screen::NotFound, false), 0x7d, false, 5);
        assert!(require_not_found_typed_resume(&completed, "FIXTURE", &good).is_ok());
        let mut suspended = completed.clone();
        suspended["suspended"] = true.into();
        assert!(require_not_found_typed_resume(&suspended, "FIXTURE", &good).is_err());
        assert!(!receive_matches(&[], 1, 375));
        assert!(!receive_matches(
            &[good[0].clone(), good[0].clone()],
            1,
            375
        ));
        for alter in 0..5 {
            let mut bad = good.clone();
            match alter {
                0 => bad[0].outcome = "MAPFAIL".into(),
                1 => bad[0].response = 36,
                2 => bad[0].response2 = 1,
                3 => bad[0].payload_bytes -= 1,
                _ => bad[0].operation = CicsOperation::SendMap,
            }
            assert!(!receive_matches(&bad, 1, 375));
        }
        assert!(no_file_effects(&good));
        let bad = [CicsTraceEntry {
            operation: CicsOperation::Read,
            outcome: "NORMAL".into(),
            response: 0,
            response2: 0,
            payload_bytes: 80,
        }];
        assert!(!no_file_effects(&bad));
        let mut observations = RouteObservations::default();
        let mut bad = fields(Screen::WrongPassword, false);
        bad.get_mut("TITLE01").unwrap()[0] ^= 1;
        let response = terminal(&bad, 0x7d, true, 5);
        assert!(
            compare_wrong_password(
                &response,
                NotFoundFieldsProof { _private: () },
                &mut observations
            )
            .is_err()
        );
        assert!(observations.journeys().is_empty());
    }
}
