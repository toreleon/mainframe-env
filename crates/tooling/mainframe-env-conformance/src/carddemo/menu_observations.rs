//! Pinned menu labels and a selected COMEN01 server-reopen comparison.
//! Label checks earn no full menu/access-filtering or XCTL-target observation.
use super::transaction_harness::RawRows;
use super::*;

const SOURCES: &[(&str, &str)] = &[
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/cbl/COSGN00C.cbl",
        "4f901ae6b113eebae60ae1d94b30f8cb9906d031a4419770bf56980bdf97e377",
    ),
    (
        "app/cbl/COMEN01C.cbl",
        "c77747b4caaf00b45177a6449562b7f59ec336db832937595edef8b32fdce6fa",
    ),
    (
        "app/cbl/COADM01C.cbl",
        "4e49afda5f685f3ebeb95ed04a93cf88df93d39c66d8d7f9ba2b9a3e00a1f690",
    ),
    (
        "app/cbl/COACTVWC.cbl",
        "4f1e55176f69edfb55b7af7a161aceb8c3bc3bd1b2924a704a76256e0e751b54",
    ),
    (
        "app/bms/COMEN01.bms",
        "f87af09e6f60ab198d5c05817e3007361a6d29ed957322ef25b11c5420c84b4c",
    ),
    (
        "app/bms/COADM01.bms",
        "44a3f209e7fa8445355ab7ba687b3ac6e32ef4dfec225e04f4eb2bd57e39386a",
    ),
    (
        "app/bms/COACTVW.bms",
        "e229c1b4c9d3f52a5b8ebbb5266c9e47502ac90577b46f5f146bdb34b05f7f46",
    ),
    (
        "app/cpy-bms/COMEN01.CPY",
        "21275363519e57f47c5c7842178344208f3ad773a551e74e5eeff285c775ad5c",
    ),
    (
        "app/cpy-bms/COADM01.CPY",
        "ced4c34fdb3fa4284561d83ff8e273cef2a1b45cefc36e56c34da6fec5528006",
    ),
    (
        "app/cpy-bms/COACTVW.CPY",
        "f3bf70a636aecb5874518ccc53f9826bcc69070b8307bdc551b2c57b9dd61dce",
    ),
    (
        "app/cpy/COMEN02Y.cpy",
        "433f5b373c2a42c3c83d62eef1dfb5b101265a9db62e42857ba58acf8c50011d",
    ),
    (
        "app/cpy/COADM02Y.cpy",
        "86d4b5b5c89449126b2a37bb46458fe451a2523490d3c7f7f9c35d36199fe6b3",
    ),
    (
        "app/cpy/COTTL01Y.cpy",
        "5032bfa9428354bb4a62e8c82ba1efa7dcf6b80f27544ad99a81c8a8df8ed41b",
    ),
    (
        "app/cpy/COCOM01Y.cpy",
        "77097557ee4c365f62138328d3e280ce7aba5f81d418c6f16434f0fbb2aafa6c",
    ),
];

// ASCII and Cp037 alternatives are frozen from first-party PIC/BMS widths.
// No observed output, product encoder or runtime formatter constructs expectations.
const REGULAR: &[(&str, &[u8], &[u8])] = &[
    (
        "PGMNAME",
        b"COMEN01C",
        &[0xc3, 0xd6, 0xd4, 0xc5, 0xd5, 0xf0, 0xf1, 0xc3],
    ),
    ("TRNNAME", b"CM00", &[0xc3, 0xd4, 0xf0, 0xf0]),
    (
        "TITLE01",
        b"      AWS Mainframe Modernization       ",
        &[
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xc1, 0xe6, 0xe2, 0x40, 0xd4, 0x81, 0x89, 0x95,
            0x86, 0x99, 0x81, 0x94, 0x85, 0x40, 0xd4, 0x96, 0x84, 0x85, 0x99, 0x95, 0x89, 0xa9,
            0x81, 0xa3, 0x89, 0x96, 0x95, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "TITLE02",
        b"              CardDemo                  ",
        &[
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0xc3, 0x81, 0x99, 0x84, 0xc4, 0x85, 0x94, 0x96, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN001",
        b"01. Account View                        ",
        &[
            0xf0, 0xf1, 0x4b, 0x40, 0xc1, 0x83, 0x83, 0x96, 0xa4, 0x95, 0xa3, 0x40, 0xe5, 0x89,
            0x85, 0xa6, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN002",
        b"02. Account Update                      ",
        &[
            0xf0, 0xf2, 0x4b, 0x40, 0xc1, 0x83, 0x83, 0x96, 0xa4, 0x95, 0xa3, 0x40, 0xe4, 0x97,
            0x84, 0x81, 0xa3, 0x85, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN003",
        b"03. Credit Card List                    ",
        &[
            0xf0, 0xf3, 0x4b, 0x40, 0xc3, 0x99, 0x85, 0x84, 0x89, 0xa3, 0x40, 0xc3, 0x81, 0x99,
            0x84, 0x40, 0xd3, 0x89, 0xa2, 0xa3, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN004",
        b"04. Credit Card View                    ",
        &[
            0xf0, 0xf4, 0x4b, 0x40, 0xc3, 0x99, 0x85, 0x84, 0x89, 0xa3, 0x40, 0xc3, 0x81, 0x99,
            0x84, 0x40, 0xe5, 0x89, 0x85, 0xa6, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN005",
        b"05. Credit Card Update                  ",
        &[
            0xf0, 0xf5, 0x4b, 0x40, 0xc3, 0x99, 0x85, 0x84, 0x89, 0xa3, 0x40, 0xc3, 0x81, 0x99,
            0x84, 0x40, 0xe4, 0x97, 0x84, 0x81, 0xa3, 0x85, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN006",
        b"06. Transaction List                    ",
        &[
            0xf0, 0xf6, 0x4b, 0x40, 0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96,
            0x95, 0x40, 0xd3, 0x89, 0xa2, 0xa3, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN007",
        b"07. Transaction View                    ",
        &[
            0xf0, 0xf7, 0x4b, 0x40, 0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96,
            0x95, 0x40, 0xe5, 0x89, 0x85, 0xa6, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN008",
        b"08. Transaction Add                     ",
        &[
            0xf0, 0xf8, 0x4b, 0x40, 0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96,
            0x95, 0x40, 0xc1, 0x84, 0x84, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN009",
        b"09. Transaction Reports                 ",
        &[
            0xf0, 0xf9, 0x4b, 0x40, 0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96,
            0x95, 0x40, 0xd9, 0x85, 0x97, 0x96, 0x99, 0xa3, 0xa2, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN010",
        b"10. Bill Payment                        ",
        &[
            0xf1, 0xf0, 0x4b, 0x40, 0xc2, 0x89, 0x93, 0x93, 0x40, 0xd7, 0x81, 0xa8, 0x94, 0x85,
            0x95, 0xa3, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN011",
        b"11. Pending Authorization View          ",
        &[
            0xf1, 0xf1, 0x4b, 0x40, 0xd7, 0x85, 0x95, 0x84, 0x89, 0x95, 0x87, 0x40, 0xc1, 0xa4,
            0xa3, 0x88, 0x96, 0x99, 0x89, 0xa9, 0x81, 0xa3, 0x89, 0x96, 0x95, 0x40, 0xe5, 0x89,
            0x85, 0xa6, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
];

const ADMIN: &[(&str, &[u8], &[u8])] = &[
    (
        "PGMNAME",
        b"COADM01C",
        &[0xc3, 0xd6, 0xc1, 0xc4, 0xd4, 0xf0, 0xf1, 0xc3],
    ),
    ("TRNNAME", b"CA00", &[0xc3, 0xc1, 0xf0, 0xf0]),
    (
        "TITLE01",
        b"      AWS Mainframe Modernization       ",
        &[
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xc1, 0xe6, 0xe2, 0x40, 0xd4, 0x81, 0x89, 0x95,
            0x86, 0x99, 0x81, 0x94, 0x85, 0x40, 0xd4, 0x96, 0x84, 0x85, 0x99, 0x95, 0x89, 0xa9,
            0x81, 0xa3, 0x89, 0x96, 0x95, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "TITLE02",
        b"              CardDemo                  ",
        &[
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0xc3, 0x81, 0x99, 0x84, 0xc4, 0x85, 0x94, 0x96, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN001",
        b"01. User List (Security)                ",
        &[
            0xf0, 0xf1, 0x4b, 0x40, 0xe4, 0xa2, 0x85, 0x99, 0x40, 0xd3, 0x89, 0xa2, 0xa3, 0x40,
            0x4d, 0xe2, 0x85, 0x83, 0xa4, 0x99, 0x89, 0xa3, 0xa8, 0x5d, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN002",
        b"02. User Add (Security)                 ",
        &[
            0xf0, 0xf2, 0x4b, 0x40, 0xe4, 0xa2, 0x85, 0x99, 0x40, 0xc1, 0x84, 0x84, 0x40, 0x4d,
            0xe2, 0x85, 0x83, 0xa4, 0x99, 0x89, 0xa3, 0xa8, 0x5d, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN003",
        b"03. User Update (Security)              ",
        &[
            0xf0, 0xf3, 0x4b, 0x40, 0xe4, 0xa2, 0x85, 0x99, 0x40, 0xe4, 0x97, 0x84, 0x81, 0xa3,
            0x85, 0x40, 0x4d, 0xe2, 0x85, 0x83, 0xa4, 0x99, 0x89, 0xa3, 0xa8, 0x5d, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN004",
        b"04. User Delete (Security)              ",
        &[
            0xf0, 0xf4, 0x4b, 0x40, 0xe4, 0xa2, 0x85, 0x99, 0x40, 0xc4, 0x85, 0x93, 0x85, 0xa3,
            0x85, 0x40, 0x4d, 0xe2, 0x85, 0x83, 0xa4, 0x99, 0x89, 0xa3, 0xa8, 0x5d, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "OPTN005",
        b"05. Transaction Type List/Update (Db2)  ",
        &[
            0xf0, 0xf5, 0x4b, 0x40, 0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96,
            0x95, 0x40, 0xe3, 0xa8, 0x97, 0x85, 0x40, 0xd3, 0x89, 0xa2, 0xa3, 0x61, 0xe4, 0x97,
            0x84, 0x81, 0xa3, 0x85, 0x40, 0x4d, 0xc4, 0x82, 0xf2, 0x5d, 0x40, 0x40,
        ],
    ),
    (
        "OPTN006",
        b"06. Transaction Type Maintenance (Db2)  ",
        &[
            0xf0, 0xf6, 0x4b, 0x40, 0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96,
            0x95, 0x40, 0xe3, 0xa8, 0x97, 0x85, 0x40, 0xd4, 0x81, 0x89, 0x95, 0xa3, 0x85, 0x95,
            0x81, 0x95, 0x83, 0x85, 0x40, 0x4d, 0xc4, 0x82, 0xf2, 0x5d, 0x40, 0x40,
        ],
    ),
];

const ACCOUNT: &[(&str, &[u8], &[u8])] = &[
    (
        "PGMNAME",
        b"COACTVWC",
        &[0xc3, 0xd6, 0xc1, 0xc3, 0xe3, 0xe5, 0xe6, 0xc3],
    ),
    ("TRNNAME", b"CAVW", &[0xc3, 0xc1, 0xe5, 0xe6]),
    (
        "TITLE01",
        b"      AWS Mainframe Modernization       ",
        &[
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xc1, 0xe6, 0xe2, 0x40, 0xd4, 0x81, 0x89, 0x95,
            0x86, 0x99, 0x81, 0x94, 0x85, 0x40, 0xd4, 0x96, 0x84, 0x85, 0x99, 0x95, 0x89, 0xa9,
            0x81, 0xa3, 0x89, 0x96, 0x95, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
    (
        "TITLE02",
        b"              CardDemo                  ",
        &[
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0xc3, 0x81, 0x99, 0x84, 0xc4, 0x85, 0x94, 0x96, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
            0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
        ],
    ),
];

fn refuse(condition: bool, detail: &str) -> Result<(), CorpusProblem> {
    if condition {
        Err(CorpusProblem::new(
            "carddemo.online.menu_restart_drift",
            detail,
        ))
    } else {
        Ok(())
    }
}

fn verify_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (relative, expected) in SOURCES {
        let bytes = read_corpus_file(corpus, &corpus.join(relative))?;
        refuse(
            format!("{:x}", Sha256::digest(&bytes)) != *expected,
            relative,
        )?;
    }
    Ok(())
}

fn identity_and_fields(
    response: &serde_json::Value,
    mapset: &str,
    map: &str,
    expected: &[(&str, &[u8], &[u8])],
) -> Result<(), CorpusProblem> {
    refuse(
        response["mapset"] != mapset || response["map"] != map,
        "wrong selected map identity",
    )?;
    let fields = online_screen_fields(response)?;
    for (name, ascii, cp037) in expected {
        refuse(
            fields
                .get(*name)
                .is_none_or(|actual| actual.as_slice() != *ascii && actual.as_slice() != *cp037),
            name,
        )?;
    }
    Ok(())
}

pub(super) fn compare_regular_menu(
    corpus: &Path,
    response: &serde_json::Value,
) -> Result<(), CorpusProblem> {
    verify_sources(corpus)?;
    identity_and_fields(response, "COMEN01", "COMEN1A", REGULAR)
}

pub(super) async fn compare_admin_menu(
    corpus: &Path,
    app: &axum::Router,
    route: &CardDemoOnlineSession,
) -> Result<(), CorpusProblem> {
    verify_sources(corpus)?;
    let response = read_terminal(app, route).await?;
    identity_and_fields(&response, "COADM01", "COADM1A", ADMIN)
}

async fn read_terminal(
    app: &axum::Router,
    route: &CardDemoOnlineSession,
) -> Result<serde_json::Value, CorpusProblem> {
    let result = terminal_http(
        app,
        Method::GET,
        &format!("/mainframe-env/cics/v1/sessions/{}", route.session),
        route.headers.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(result.0, StatusCode::OK, "selected menu read-back")?;
    serde_json::from_slice(&result.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))
}

// No PUT is offered: an absent input is the source RECEIVE suspension contract.
// An explicitly submitted empty field map would instead be ordinary input success.
async fn suspend_without_input(
    server: &ProductServer,
    app: &axum::Router,
    route: &CardDemoOnlineSession,
    mapset: &str,
    map: &str,
    expected: &[(&str, &[u8], &[u8])],
    operations: &[CicsOperation],
) -> Result<serde_json::Value, CorpusProblem> {
    let completed = read_terminal(app, route).await?;
    identity_and_fields(&completed, mapset, map, expected)?;
    refuse(
        completed["session"] != route.session
            || completed["principal"] != "WEBUSER"
            || completed["connected"] != true
            || completed["suspended"] != false,
        "inputless resume did not start from the selected ordinary RETURN stage",
    )?;
    let before = server
        .online_trace(&route.session)
        .map_err(terminal_problem)?;
    let result = terminal_http(
        app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{}/resume", route.session),
        route.headers.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(
        result.0,
        StatusCode::OK,
        "genuine inputless RECEIVE suspension",
    )?;
    let resumed: serde_json::Value = serde_json::from_slice(&result.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    identity_and_fields(&resumed, mapset, map, expected)?;
    let after = server
        .online_trace(&route.session)
        .map_err(terminal_problem)?;
    refuse(
        !after.starts_with(&before)
            || after.len() != before.len() + operations.len()
            || after[before.len()..]
                .iter()
                .zip(operations)
                .any(|(actual, expected)| {
                    actual.operation != *expected
                        || actual.outcome != "NORMAL"
                        || actual.response != 0
                        || actual.response2 != 0
                        || actual.payload_bytes != 0
                }),
        "inputless resume differs from its exact source command/condition suffix",
    )?;
    let persisted = read_terminal(app, route).await?;
    identity_and_fields(&persisted, mapset, map, expected)?;
    addressed_suspension_response(&resumed, &persisted, &route.session)?;
    refuse(
        resumed["suspended"] != true
            || persisted["session"] != route.session
            || persisted["principal"] != "WEBUSER"
            || persisted["connected"] != true
            || persisted["suspended"] != true
            || persisted["version"].as_u64().is_none_or(|version| {
                completed["version"]
                    .as_u64()
                    .is_none_or(|prior| version <= prior)
            }),
        "actual absent-input RECEIVE did not persist an addressed newer suspended state",
    )?;
    Ok(persisted)
}

fn addressed_suspension_response(
    resumed: &serde_json::Value,
    persisted: &serde_json::Value,
    session: &str,
) -> Result<(), CorpusProblem> {
    refuse(
        resumed["session"] != session
            || resumed["principal"] != "WEBUSER"
            || resumed["connected"] != true
            || resumed["suspended"] != true
            || resumed["version"]
                .as_u64()
                .is_none_or(|version| version == 0)
            || resumed != persisted,
        "actual resume response differs from the whole addressed persisted suspended state",
    )
}

fn suspended_menu(response: &serde_json::Value, session: &str) -> Result<(), CorpusProblem> {
    identity_and_fields(response, "COMEN01", "COMEN1A", REGULAR)?;
    refuse(
        response["session"] != session
            || response["principal"] != "WEBUSER"
            || response["suspended"] != true
            || response["connected"] != true
            || response["version"]
                .as_u64()
                .is_none_or(|version| version == 0),
        "menu did not retain an addressed suspended principal/version",
    )
}

fn compare_reopened_menu(
    before: &serde_json::Value,
    reopened: &serde_json::Value,
    session: &str,
) -> Result<(), CorpusProblem> {
    suspended_menu(before, session)?;
    suspended_menu(reopened, session)?;
    // Equality proves the actual retained menu/version/run/session state.
    // Independently frozen literals above supply the source-owned values.
    refuse(
        before != reopened,
        "reopened menu differs from its durable selected state",
    )
}

fn compare_account_resume(
    response: &serde_json::Value,
    session: &str,
    before: &[RawRows],
    after: &[RawRows],
) -> Result<(), CorpusProblem> {
    identity_and_fields(response, "COACTVW", "CACTVWA", ACCOUNT)?;
    refuse(
        response["session"] != session
            || response["principal"] != "WEBUSER"
            || response["suspended"] != true
            || response["connected"] != true
            || before.len() != 5
            || after != before,
        "selected option1 did not preserve its session and complete selected raw datasets",
    )
}

pub(super) struct RestartMenu {
    route: CardDemoOnlineSession,
    menu: serde_json::Value,
    before: Vec<RawRows>,
}

impl RestartMenu {
    pub(super) async fn prepare(
        corpus: &Path,
        server: &ProductServer,
        app: &axum::Router,
    ) -> Result<Self, CorpusProblem> {
        verify_sources(corpus)?;
        let before = transaction_harness::snapshot(server)?;
        let route = open_carddemo_menu(
            server,
            app,
            "WEBUSER",
            "transport-password",
            "USER0001",
            "PASSWORD",
            "COMEN01",
        )
        .await?;
        let menu = suspend_without_input(
            server,
            app,
            &route,
            "COMEN01",
            "COMEN1A",
            REGULAR,
            &[CicsOperation::ReceiveMap],
        )
        .await?;
        suspended_menu(&menu, &route.session)?;
        refuse(
            transaction_harness::snapshot(server)? != before,
            "pre-reopen menu changed raw dataset records/keys/versions",
        )?;
        Ok(Self {
            route,
            menu,
            before,
        })
    }

    pub(super) async fn resume(
        self,
        server: &ProductServer,
        app: &axum::Router,
        observations: &mut RouteObservations,
    ) -> Result<(), CorpusProblem> {
        let reopened = read_terminal(app, &self.route).await?;
        compare_reopened_menu(&self.menu, &reopened, &self.route.session)?;
        refuse(
            transaction_harness::snapshot(server)? != self.before,
            "reopen changed raw dataset records/keys/versions",
        )?;
        let selected = carddemo_terminal_exchange(
            app,
            &self.route.session,
            &self.route.headers,
            0x7d,
            BTreeMap::from([("OPTION".into(), "1".into())]),
        )
        .await?;
        // Keep the original selected account identity/literal checks before any
        // further resume, then reach its genuine source RECEIVE suspension.
        identity_and_fields(&selected, "COACTVW", "CACTVWA", ACCOUNT)?;
        let suspended = suspend_without_input(
            server,
            app,
            &self.route,
            "COACTVW",
            "CACTVWA",
            ACCOUNT,
            &[CicsOperation::HandleAbend, CicsOperation::ReceiveMap],
        )
        .await?;
        let after = transaction_harness::snapshot(server)?;
        let comparison =
            compare_account_resume(&suspended, &self.route.session, &self.before, &after);
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J03",
            "restart from suspended menu",
            comparison.is_err(),
            || {
                comparison.err().ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.online.menu_restart_drift",
                        "missing refusal diagnostic",
                    )
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(
        mapset: &str,
        map: &str,
        literals: &[(&str, &[u8], &[u8])],
        ebcdic: bool,
    ) -> serde_json::Value {
        let mut screen = Vec::new();
        for (name, ascii, cp037) in literals {
            let value = if ebcdic { cp037 } else { ascii };
            screen.extend_from_slice(&(name.len() as u32).to_be_bytes());
            screen.extend_from_slice(name.as_bytes());
            screen.extend_from_slice(&(value.len() as u32).to_be_bytes());
            screen.extend_from_slice(value);
        }
        serde_json::json!({
            "mapset":mapset,"map":map,"session":"selected-menu","principal":"WEBUSER",
            "suspended":true,"connected":true,"version":7,
            "screen_base64":base64::engine::general_purpose::STANDARD.encode(screen)
        })
    }

    #[test]
    fn menu_label_byte_padding_slot_and_identity_drift_refuse() {
        for ebcdic in [false, true] {
            let correct = response("COMEN01", "COMEN1A", REGULAR, ebcdic);
            assert!(suspended_menu(&correct, "selected-menu").is_ok());
            let mut wrong = correct.clone();
            wrong["map"] = "COADM1A".into();
            assert!(suspended_menu(&wrong, "selected-menu").is_err());
            let mut swapped = REGULAR.to_vec();
            let first = swapped[4];
            let second = swapped[5];
            swapped[4] = (first.0, second.1, second.2);
            swapped[5] = (second.0, first.1, first.2);
            assert!(
                suspended_menu(
                    &response("COMEN01", "COMEN1A", &swapped, ebcdic),
                    "selected-menu"
                )
                .is_err()
            );
            let mut short = REGULAR.to_vec();
            short[4] = (short[4].0, &short[4].1[..39], &short[4].2[..39]);
            assert!(
                suspended_menu(
                    &response("COMEN01", "COMEN1A", &short, ebcdic),
                    "selected-menu"
                )
                .is_err()
            );
            assert!(
                suspended_menu(
                    &response("COMEN01", "COMEN1A", &REGULAR[..REGULAR.len() - 1], ebcdic),
                    "selected-menu"
                )
                .is_err()
            );
            let mut raw = base64::engine::general_purpose::STANDARD
                .decode(correct["screen_base64"].as_str().unwrap())
                .unwrap();
            *raw.last_mut().unwrap() ^= 1;
            wrong = correct.clone();
            wrong["screen_base64"] = base64::engine::general_purpose::STANDARD.encode(raw).into();
            assert!(suspended_menu(&wrong, "selected-menu").is_err());
        }
    }

    #[test]
    fn actual_resume_response_foreign_identity_version_or_run_refuses() {
        let mut correct = response("COMEN01", "COMEN1A", REGULAR, false);
        correct["run_unit"] = serde_json::json!("actual-source-run");
        correct["transaction"] = serde_json::json!("CM00");
        assert!(addressed_suspension_response(&correct, &correct, "selected-menu").is_ok());
        for (name, value) in [
            ("session", serde_json::json!("other")),
            ("principal", serde_json::json!("WEBADM")),
            ("connected", serde_json::json!(false)),
            ("suspended", serde_json::json!(false)),
            ("version", serde_json::json!(8)),
            ("version", serde_json::json!(0)),
            ("run_unit", serde_json::json!("foreign-source-run")),
            ("transaction", serde_json::json!("CA00")),
        ] {
            let mut wrong = correct.clone();
            wrong[name] = value;
            assert!(
                addressed_suspension_response(&wrong, &correct, "selected-menu").is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn reopened_menu_version_principal_session_or_suspension_drift_refuse() {
        let correct = response("COMEN01", "COMEN1A", REGULAR, false);
        for (name, value) in [
            ("version", serde_json::json!(8)),
            ("principal", serde_json::json!("WEBADM")),
            ("session", serde_json::json!("other")),
            ("suspended", serde_json::json!(false)),
            ("connected", serde_json::json!(false)),
        ] {
            let mut reopened = correct.clone();
            reopened[name] = value;
            assert!(
                compare_reopened_menu(&correct, &reopened, "selected-menu").is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn account_resume_wrong_target_or_raw_byte_key_version_refuses() {
        let correct = response("COACTVW", "CACTVWA", ACCOUNT, false);
        let state: Vec<RawRows> = (0..5)
            .map(|_| (vec![vec![1, 2, 3]], vec![vec![1]], 1))
            .collect();
        assert!(compare_account_resume(&correct, "selected-menu", &state, &state).is_ok());
        let wrong = response("COMEN01", "COMEN1A", REGULAR, false);
        assert!(compare_account_resume(&wrong, "selected-menu", &state, &state).is_err());
        for component in 0..3 {
            let mut after = state.clone();
            match component {
                0 => after[0].0[0][0] ^= 1,
                1 => after[0].1[0][0] ^= 1,
                _ => after[0].2 += 1,
            }
            assert!(compare_account_resume(&correct, "selected-menu", &state, &after).is_err());
        }
        assert!(
            compare_account_resume(&correct, "selected-menu", &state[..4], &state[..4]).is_err()
        );
    }
}
