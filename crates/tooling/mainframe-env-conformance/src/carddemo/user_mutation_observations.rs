//! Exact first-party J08 user lifecycle and exercised application list pages.
// CardDemo 59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e: CSUSR01Y defines
// ID8/FNAME20/LNAME20/PWD8/TYPE1/FILLER23. The ten seed rows, keys and
// controlled TEST0001 add/update rows are independent frozen 80-byte Cp037
// literals. No runtime encoder or provider result supplies expected bytes.
// COUSR01C does not overwrite FILLER. Its expected 23 spaces use the existing
// local alphanumeric initialization contract; this earns no new IBM semantics.
// Every CRUD phase compares all ten seeds plus the controlled row, all raw keys,
// fixed-80/CCSID37/key0+8 attributes, catalog relationship and exact +1 version.
// The full installed 12-dataset + 3-index inventory is captured without a
// truncated read; every other row, identity, layout, relationship and revision
// must remain unchanged. This is publication-state evidence, not crash atomicity.
// COUSR00C's ten-row source page uses ENTER, PF8 and PF7 after the existing add:
// ten rows / one row + nine blank slots / restored ten rows. No new fixture,
// row-selector mutation, full-map, RACF or licensed IBM parity credit is earned.
use super::*;
use mainframe_env_cics::CicsTraceEntry;
use mainframe_env_host_api::{CatalogEntryKind, CatalogListEntry};

const USER_DATASET: &str = "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS";
const DATASETS: [&str; 15] = [
    "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDDATA.VSAM.AIX.PATH",
    "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.DALYTRAN.PS",
    "AWS.M2.CARDDEMO.DISCGRP.VSAM.KSDS",
    "AWS.M2.CARDDEMO.EXPORT.DATA",
    "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
    "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS",
    USER_DATASET,
];

const SEED_RECORDS: [&[u8]; 10] = [
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf1\xd4\xc1\xd9\xc7\xc1\xd9\xc5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xc7\xd6\xd3\xc4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xc1\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf2\xd9\xe4\xe2\xe2\xc5\xd3\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd9\xe4\xe2\xe2\xc5\xd3\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xc1\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf3\xd9\xc1\xe8\xd4\xd6\xd5\xc4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe6\xc8\xc9\xe3\xd4\xd6\xd9\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xc1\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf4\xc5\xd4\xd4\xc1\xd5\xe4\xc5\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xc3\xc1\xe2\xc7\xd9\xc1\xc9\xd5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xc1\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf5\xc7\xd9\xc1\xd5\xe5\xc9\xd3\xd3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd3\xc1\xc3\xc8\xc1\xd7\xc5\xd3\xd3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xc1\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf1\xd3\xc1\xe6\xd9\xc5\xd5\xc3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe3\xc8\xd6\xd4\xc1\xe2\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf2\xc1\xd1\xc9\xe3\xc8\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd2\xe4\xd4\xc1\xd9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf3\xd3\xc1\xe4\xd9\xc9\xe3\xe9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xc1\xd3\xd4\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf4\xc1\xe5\xc5\xd9\xc1\xd9\xc4\xd6\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd4\xc1\xe9\xe9\xc9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf5\xd3\xc5\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe3\xc9\xd5\xc7\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd7\xc1\xe2\xe2\xe6\xd6\xd9\xc4\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
];

const SEED_KEYS: [&[u8]; 10] = [
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf1",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf2",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf3",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf4",
    b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf5",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf1",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf2",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf3",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf4",
    b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf5",
];

const ADDED_RECORD: &[u8] = b"\xe3\xc5\xe2\xe3\xf0\xf0\xf0\xf1\xe3\xc5\xe2\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xd6\xd7\xc5\xd9\xc1\xe3\xd6\xd9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe2\xc5\xc3\xd9\xc5\xe3\xd7\xf1\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40";
const UPDATED_RECORD: &[u8] = b"\xe3\xc5\xe2\xe3\xf0\xf0\xf0\xf1\xe3\xc5\xe2\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe4\xd7\xc4\xc1\xe3\xc5\xc4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe2\xc5\xc3\xd9\xc5\xe3\xd7\xf1\xe4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40";
const CONTROLLED_KEY: &[u8] = b"\xe3\xc5\xe2\xe3\xf0\xf0\xf0\xf1";

const LIST_ROWS: [[(&[u8], &[u8]); 4]; 11] = [
    [
        (b"ADMIN001", b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf1"),
        (
            b"MARGARET            ",
            b"\xd4\xc1\xd9\xc7\xc1\xd9\xc5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"GOLD                ",
            b"\xc7\xd6\xd3\xc4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"A", b"\xc1"),
    ],
    [
        (b"ADMIN002", b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf2"),
        (
            b"RUSSELL             ",
            b"\xd9\xe4\xe2\xe2\xc5\xd3\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"RUSSELL             ",
            b"\xd9\xe4\xe2\xe2\xc5\xd3\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"A", b"\xc1"),
    ],
    [
        (b"ADMIN003", b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf3"),
        (
            b"RAYMOND             ",
            b"\xd9\xc1\xe8\xd4\xd6\xd5\xc4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"WHITMORE            ",
            b"\xe6\xc8\xc9\xe3\xd4\xd6\xd9\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"A", b"\xc1"),
    ],
    [
        (b"ADMIN004", b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf4"),
        (
            b"EMMANUEL            ",
            b"\xc5\xd4\xd4\xc1\xd5\xe4\xc5\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"CASGRAIN            ",
            b"\xc3\xc1\xe2\xc7\xd9\xc1\xc9\xd5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"A", b"\xc1"),
    ],
    [
        (b"ADMIN005", b"\xc1\xc4\xd4\xc9\xd5\xf0\xf0\xf5"),
        (
            b"GRANVILLE           ",
            b"\xc7\xd9\xc1\xd5\xe5\xc9\xd3\xd3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"LACHAPELLE          ",
            b"\xd3\xc1\xc3\xc8\xc1\xd7\xc5\xd3\xd3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"A", b"\xc1"),
    ],
    [
        (b"TEST0001", b"\xe3\xc5\xe2\xe3\xf0\xf0\xf0\xf1"),
        (
            b"TEST                ",
            b"\xe3\xc5\xe2\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"OPERATOR            ",
            b"\xd6\xd7\xc5\xd9\xc1\xe3\xd6\xd9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"U", b"\xe4"),
    ],
    [
        (b"USER0001", b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf1"),
        (
            b"LAWRENCE            ",
            b"\xd3\xc1\xe6\xd9\xc5\xd5\xc3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"THOMAS              ",
            b"\xe3\xc8\xd6\xd4\xc1\xe2\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"U", b"\xe4"),
    ],
    [
        (b"USER0002", b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf2"),
        (
            b"AJITH               ",
            b"\xc1\xd1\xc9\xe3\xc8\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"KUMAR               ",
            b"\xd2\xe4\xd4\xc1\xd9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"U", b"\xe4"),
    ],
    [
        (b"USER0003", b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf3"),
        (
            b"LAURITZ             ",
            b"\xd3\xc1\xe4\xd9\xc9\xe3\xe9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"ALME                ",
            b"\xc1\xd3\xd4\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"U", b"\xe4"),
    ],
    [
        (b"USER0004", b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf4"),
        (
            b"AVERARDO            ",
            b"\xc1\xe5\xc5\xd9\xc1\xd9\xc4\xd6\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"MAZZI               ",
            b"\xd4\xc1\xe9\xe9\xc9\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"U", b"\xe4"),
    ],
    [
        (b"USER0005", b"\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf5"),
        (
            b"LEE                 ",
            b"\xd3\xc5\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (
            b"TING                ",
            b"\xe3\xc9\xd5\xc7\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
        ),
        (b"U", b"\xe4"),
    ],
];
const BLANK_ROW: [(&[u8], &[u8]); 4] = [
    (b"        ", b"\x40\x40\x40\x40\x40\x40\x40\x40"),
    (
        b"                    ",
        b"\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    ),
    (
        b"                    ",
        b"\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40",
    ),
    (b" ", b"\x40"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Rows {
    attributes: DatasetAttributes,
    attributes_version: u64,
    records: Vec<Vec<u8>>,
    identities: Vec<Vec<u8>>,
    version: u64,
    catalog: CatalogListEntry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Snapshot(BTreeMap<String, Rows>);

#[derive(Clone, Copy)]
enum Phase {
    Seed,
    Added,
    Updated,
}

#[derive(Clone, Copy)]
pub(super) enum Mutation {
    Add,
    Update,
    Delete,
}

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.user_raw_state_drift", detail)
}

pub(super) fn verify_sources() -> Result<(), CorpusProblem> {
    security_observations::verify_user_security_sources()?;
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("verified corpus environment"));
    for (relative, expected) in [
        (
            "app/cpy-bms/COUSR00.CPY",
            "88c9031484d3bd2da62488c6941c431ee3b910b3feaf7918762968ba0bb9c318",
        ),
        (
            "app/cpy/COCOM01Y.cpy",
            "77097557ee4c365f62138328d3e280ce7aba5f81d418c6f16434f0fbb2aafa6c",
        ),
    ] {
        let bytes = read_corpus_file(&corpus, &corpus.join(relative))?;
        if format!("{:x}", Sha256::digest(&bytes)) != expected {
            return Err(drift(format!(
                "pinned user layout source changed: {relative}"
            )));
        }
    }
    Ok(())
}

pub(super) fn capture(server: &ProductServer) -> Result<Snapshot, CorpusProblem> {
    let entries = match server
        .dataset_service()
        .invoke(DatasetRequest::ListCatalog {
            pattern: "AWS.M2.CARDDEMO.*".into(),
            start: None,
            max_items: 16,
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::CatalogEntries {
            entries,
            more: false,
        } if entries.len() == 15 => entries,
        _ => {
            return Err(drift(
                "installed user inventory is incomplete or has an extra object",
            ));
        }
    };
    let mut snapshot = BTreeMap::new();
    for catalog in entries {
        let name = catalog.name.clone();
        let (attributes, attributes_version) = match server
            .dataset_service()
            .invoke(DatasetRequest::Attributes {
                dataset: name.clone(),
            })
            .map_err(terminal_problem)?
        {
            DatasetResult::Attributes {
                attributes,
                version,
            } => (attributes, version),
            _ => {
                return Err(drift(
                    "installed user inventory attributes returned the wrong result",
                ));
            }
        };
        let (records, identities, version) = match server
            .dataset_service()
            .invoke(DatasetRequest::Read {
                dataset: name.clone(),
                member: None,
                key: None,
                max_records: 4096,
                control: Default::default(),
            })
            .map_err(terminal_problem)?
        {
            // Preserve the existing bounded read limit; a full sentinel result
            // is refused rather than silently claiming a complete comparison.
            DatasetResult::Records {
                records,
                identities,
                version,
            } if records.len() < 4096 => (records, identities, version),
            _ => {
                return Err(drift(
                    "installed user inventory read is incomplete or invalid",
                ));
            }
        };
        if version == 0 || version != attributes_version || version != catalog.version {
            return Err(drift("installed user inventory revisions disagree"));
        }
        let rows = Rows {
            attributes,
            attributes_version,
            records,
            identities,
            version,
            catalog,
        };
        if snapshot.insert(name.as_str().into(), rows).is_some() {
            return Err(drift("installed user inventory repeated a dataset name"));
        }
    }
    let snapshot = Snapshot(snapshot);
    if !inventory_matches(&snapshot) {
        return Err(drift(
            "installed user inventory names differ from the admitted fixture",
        ));
    }
    Ok(snapshot)
}

fn inventory_matches(snapshot: &Snapshot) -> bool {
    snapshot.0.keys().map(String::as_str).eq(DATASETS)
        && snapshot.0.iter().all(|(name, rows)| {
            rows.catalog.name.as_str() == name.as_str()
                && rows.version != 0
                && rows.attributes_version == rows.version
                && rows.catalog.version == rows.version
        })
}

fn expected_rows(phase: Phase) -> Vec<&'static [u8]> {
    let mut rows = SEED_RECORDS.to_vec();
    match phase {
        Phase::Seed => (),
        Phase::Added => rows.insert(5, ADDED_RECORD),
        Phase::Updated => rows.insert(5, UPDATED_RECORD),
    }
    rows
}

fn user_matches(snapshot: &Snapshot, phase: Phase) -> bool {
    let Some(rows) = snapshot.0.get(USER_DATASET) else {
        return false;
    };
    let mut keys = SEED_KEYS.to_vec();
    if !matches!(phase, Phase::Seed) {
        keys.insert(5, CONTROLLED_KEY);
    }
    inventory_matches(snapshot)
        && rows.attributes.organization == DatasetOrganization::KeySequenced
        && rows.attributes.record_format == RecordFormat::Fixed
        && rows.attributes.logical_record_length == 80
        && rows.attributes.key_offset == Some(0)
        && rows.attributes.key_length == Some(8)
        && rows.attributes.ccsid == Some(37)
        && rows.catalog.kind == CatalogEntryKind::Dataset
        && rows.catalog.related.is_none()
        && rows
            .records
            .iter()
            .map(Vec::as_slice)
            .eq(expected_rows(phase))
        && rows.identities.iter().map(Vec::as_slice).eq(keys)
}

pub(super) fn require_seed(snapshot: &Snapshot) -> Result<(), CorpusProblem> {
    if !user_matches(snapshot, Phase::Seed) {
        return Err(drift(
            "all ten raw user seeds, keys, layout or revisions differ",
        ));
    }
    Ok(())
}

pub(super) fn require_unchanged(before: &Snapshot, after: &Snapshot) -> Result<(), CorpusProblem> {
    if !inventory_matches(before) || after != before {
        return Err(drift(
            "a read or refused user operation changed complete installed state",
        ));
    }
    Ok(())
}

fn file_operation(operation: CicsOperation) -> bool {
    matches!(
        operation,
        CicsOperation::Read
            | CicsOperation::Write
            | CicsOperation::Rewrite
            | CicsOperation::Delete
            | CicsOperation::StartBrowse
            | CicsOperation::ResetBrowse
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::EndBrowse
            | CicsOperation::Unlock
    )
}

type Effect = (CicsOperation, &'static str, i32, i32, usize);

fn normal(operation: CicsOperation, payload: usize) -> Effect {
    (operation, "NORMAL", 0, 0, payload)
}

fn trace_matches(trace: &[CicsTraceEntry], expected: &[Effect]) -> bool {
    trace
        .iter()
        .filter(|entry| file_operation(entry.operation))
        .map(|entry| {
            (
                entry.operation,
                entry.outcome.as_str(),
                entry.response,
                entry.response2,
                entry.payload_bytes,
            )
        })
        .eq(expected.iter().copied())
}

fn mutation_effects(mutation: Mutation) -> Vec<Effect> {
    match mutation {
        Mutation::Add => vec![normal(CicsOperation::Write, 0)],
        Mutation::Update => vec![
            normal(CicsOperation::Read, 80),
            normal(CicsOperation::Rewrite, 0),
        ],
        Mutation::Delete => vec![
            normal(CicsOperation::Read, 80),
            normal(CicsOperation::Delete, 0),
        ],
    }
}

fn transition_matches(
    mutation: Mutation,
    before: &Snapshot,
    after: &Snapshot,
    trace: &[CicsTraceEntry],
) -> bool {
    let (before_phase, after_phase) = match mutation {
        Mutation::Add => (Phase::Seed, Phase::Added),
        Mutation::Update => (Phase::Added, Phase::Updated),
        Mutation::Delete => (Phase::Updated, Phase::Seed),
    };
    if !user_matches(before, before_phase) || !user_matches(after, after_phase) {
        return false;
    }
    let before_user = &before.0[USER_DATASET];
    let after_user = &after.0[USER_DATASET];
    before_user.attributes == after_user.attributes
        && before_user.version.checked_add(1) == Some(after_user.version)
        && before
            .0
            .iter()
            .all(|(name, rows)| name.as_str() == USER_DATASET || after.0.get(name) == Some(rows))
        && trace_matches(trace, &mutation_effects(mutation))
}

pub(super) fn require_transition(
    mutation: Mutation,
    before: &Snapshot,
    after: &Snapshot,
    trace: &[CicsTraceEntry],
) -> Result<(), CorpusProblem> {
    if !transition_matches(mutation, before, after, trace) {
        return Err(drift(
            "complete user lifecycle row/key/layout/revision/effect or unrelated dataset state differs",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Page {
    First,
    Second,
    Restored,
}

fn row_field(name: &str) -> bool {
    ["USRID", "FNAME", "LNAME", "UTYPE"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|suffix| !suffix.is_empty() && suffix != "IN")
    })
}

fn list_fields_match(fields: &BTreeMap<String, Vec<u8>>, page: Page) -> bool {
    if fields.keys().filter(|name| row_field(name)).count() != 40 {
        return false;
    }
    let page_values: (&[u8], &[u8]) = match page {
        Page::Second => (b"00000002", b"\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf2"),
        _ => (b"00000001", b"\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf1"),
    };
    fields
        .get("PAGENUM")
        .is_some_and(|value| value.as_slice() == page_values.0 || value.as_slice() == page_values.1)
        && (0..10).all(|slot| {
            let expected = match page {
                Page::Second if slot == 0 => &LIST_ROWS[10],
                Page::Second => &BLANK_ROW,
                _ => &LIST_ROWS[slot],
            };
            ["USRID", "FNAME", "LNAME", "UTYPE"]
                .into_iter()
                .enumerate()
                .all(|(column, prefix)| {
                    fields
                        .get(&format!("{prefix}{:02}", slot + 1))
                        .is_some_and(|actual| {
                            let (ascii, cp037) = expected[column];
                            actual.as_slice() == ascii || actual.as_slice() == cp037
                        })
                })
        })
}

fn page_effects(page: Page) -> Vec<Effect> {
    let (operation, normal_reads, end) = match page {
        Page::First => (CicsOperation::ReadNext, 11, false),
        Page::Second => (CicsOperation::ReadNext, 2, true),
        Page::Restored => (CicsOperation::ReadPrev, 11, true),
    };
    let mut expected = vec![normal(CicsOperation::StartBrowse, 0)];
    expected.extend(std::iter::repeat_n(normal(operation, 80), normal_reads));
    if end {
        expected.push((operation, "ENDFILE", 20, 0, 0));
    }
    expected.push(normal(CicsOperation::EndBrowse, 0));
    expected
}

fn page_matches(
    page: Page,
    response: &serde_json::Value,
    trace: &[CicsTraceEntry],
    before: &Snapshot,
    after: &Snapshot,
) -> Result<bool, CorpusProblem> {
    Ok(response["mapset"] == "COUSR00"
        && response["map"] == "COUSR0A"
        && list_fields_match(&online_screen_fields(response)?, page)
        && user_matches(before, Phase::Added)
        && after == before
        && trace_matches(trace, &page_effects(page)))
}

pub(super) async fn exercise_list_cursor(
    server: &ProductServer,
    app: &axum::Router,
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    verify_sources()?;
    let before = capture(server)?;
    let route = select_admin_option(server, app, 1, "COUSR00").await?;
    require_unchanged(&before, &capture(server)?)?;
    // Actual application re-entry, then source PF8 and PF7; no synthetic
    // provider cursor substitutes for these terminal exchanges.
    for (aid, page) in [
        (0x7d, Page::First),
        (0xf8, Page::Second),
        (0xf7, Page::Restored),
    ] {
        let trace_before = server
            .online_trace(&route.session)
            .map_err(terminal_problem)?
            .len();
        let response =
            carddemo_terminal_exchange(app, &route.session, &route.headers, aid, BTreeMap::new())
                .await?;
        let trace = server
            .online_trace(&route.session)
            .map_err(terminal_problem)?;
        let fresh = trace
            .get(trace_before..)
            .ok_or_else(|| drift("actual user list trace shrank"))?;
        let after = capture(server)?;
        if !page_matches(page, &response, fresh, &before, &after)? {
            return Err(drift(
                "actual user list page/key order, fields, cursor effects or installed state differs",
            ));
        }
    }
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J08",
        "USRSEC browse",
        false,
        || Ok(drift("actual first-party user browse proof failed")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(phase: Phase, version: u64) -> Snapshot {
        Snapshot(
            DATASETS
                .into_iter()
                .map(|name| {
                    let user = name == USER_DATASET;
                    let version = if user { version } else { 7 };
                    let attributes = DatasetAttributes {
                        organization: DatasetOrganization::KeySequenced,
                        record_format: RecordFormat::Fixed,
                        logical_record_length: if user { 80 } else { 1 },
                        key_offset: Some(0),
                        key_length: Some(if user { 8 } else { 1 }),
                        ccsid: Some(37),
                    };
                    let mut keys = SEED_KEYS.to_vec();
                    if !matches!(phase, Phase::Seed) {
                        keys.insert(5, CONTROLLED_KEY);
                    }
                    (
                        name.into(),
                        Rows {
                            attributes,
                            attributes_version: version,
                            records: if user {
                                expected_rows(phase)
                                    .into_iter()
                                    .map(<[u8]>::to_vec)
                                    .collect()
                            } else {
                                vec![vec![0x40]]
                            },
                            identities: if user {
                                keys.into_iter().map(<[u8]>::to_vec).collect()
                            } else {
                                vec![vec![0x40]]
                            },
                            version,
                            catalog: CatalogListEntry {
                                name: DatasetName::new(name, 128).unwrap(),
                                kind: CatalogEntryKind::Dataset,
                                related: None,
                                version,
                            },
                        },
                    )
                })
                .collect(),
        )
    }

    fn traces(effects: &[Effect]) -> Vec<CicsTraceEntry> {
        effects
            .iter()
            .map(
                |(operation, condition, response, response2, payload_bytes)| CicsTraceEntry {
                    operation: *operation,
                    outcome: (*condition).into(),
                    response: *response,
                    response2: *response2,
                    payload_bytes: *payload_bytes,
                },
            )
            .collect()
    }

    #[test]
    fn lifecycle_requires_all_raw_bytes_keys_and_exact_revisions() {
        for (mutation, before_phase, after_phase) in [
            (Mutation::Add, Phase::Seed, Phase::Added),
            (Mutation::Update, Phase::Added, Phase::Updated),
            (Mutation::Delete, Phase::Updated, Phase::Seed),
        ] {
            let before = snapshot(before_phase, 3);
            let after = snapshot(after_phase, 4);
            let trace = traces(&mutation_effects(mutation));
            assert!(transition_matches(mutation, &before, &after, &trace));
            for row in 0..after.0[USER_DATASET].records.len() {
                for byte in 0..80 {
                    let mut bad = after.clone();
                    bad.0.get_mut(USER_DATASET).unwrap().records[row][byte] ^= 1;
                    assert!(
                        !transition_matches(mutation, &before, &bad, &trace),
                        "row={row}, byte={byte}"
                    );
                }
                for byte in 0..8 {
                    let mut bad = after.clone();
                    bad.0.get_mut(USER_DATASET).unwrap().identities[row][byte] ^= 1;
                    assert!(!transition_matches(mutation, &before, &bad, &trace));
                }
            }
            for version in [0, 3, 5] {
                let mut bad = after.clone();
                let rows = bad.0.get_mut(USER_DATASET).unwrap();
                rows.version = version;
                rows.attributes_version = version;
                rows.catalog.version = version;
                assert!(!transition_matches(mutation, &before, &bad, &trace));
            }
            let mut bad = after.clone();
            bad.0
                .get_mut(USER_DATASET)
                .unwrap()
                .records
                .push(ADDED_RECORD.to_vec());
            assert!(!transition_matches(mutation, &before, &bad, &trace));
            let mut bad = after.clone();
            bad.0
                .get_mut(USER_DATASET)
                .unwrap()
                .identities
                .push(CONTROLLED_KEY.to_vec());
            assert!(!transition_matches(mutation, &before, &bad, &trace));
            let mut bad = after.clone();
            bad.0.get_mut(USER_DATASET).unwrap().identities.pop();
            assert!(!transition_matches(mutation, &before, &bad, &trace));
            let mut bad = after.clone();
            bad.0.get_mut(USER_DATASET).unwrap().records.pop();
            assert!(!transition_matches(mutation, &before, &bad, &trace));
            let mut bad = after.clone();
            bad.0.get_mut(USER_DATASET).unwrap().records.swap(0, 1);
            bad.0.get_mut(USER_DATASET).unwrap().identities.swap(0, 1);
            assert!(!transition_matches(mutation, &before, &bad, &trace));
            let mut bad_before = before.clone();
            bad_before.0.get_mut(USER_DATASET).unwrap().records[0][79] ^= 1;
            assert!(!transition_matches(mutation, &bad_before, &after, &trace));
        }
    }

    #[test]
    fn lifecycle_rejects_layout_catalog_and_every_unrelated_dataset_change() {
        let before = snapshot(Phase::Seed, 3);
        let after = snapshot(Phase::Added, 4);
        let trace = traces(&mutation_effects(Mutation::Add));
        for alter in 0..12 {
            let mut bad = after.clone();
            let user = bad.0.get_mut(USER_DATASET).unwrap();
            match alter {
                0 => user.attributes.logical_record_length = 81,
                1 => user.attributes.key_offset = Some(1),
                2 => user.attributes.key_length = Some(7),
                3 => user.attributes.ccsid = Some(1047),
                4 => user.attributes_version -= 1,
                5 => user.catalog.version -= 1,
                6 => user.catalog.kind = CatalogEntryKind::AlternateIndex,
                7 => user.catalog.related = Some(DatasetName::new(DATASETS[0], 128).unwrap()),
                8 => user.catalog.name = DatasetName::new(DATASETS[0], 128).unwrap(),
                9 => user.attributes.organization = DatasetOrganization::Sequential,
                10 => user.attributes.record_format = RecordFormat::Variable,
                _ => user.attributes.ccsid = None,
            }
            assert!(!transition_matches(Mutation::Add, &before, &bad, &trace));
        }
        for name in DATASETS.into_iter().filter(|name| *name != USER_DATASET) {
            for alter in 0..5 {
                let mut bad = after.clone();
                let other = bad.0.get_mut(name).unwrap();
                match alter {
                    0 => other.records[0][0] ^= 1,
                    1 => other.identities[0][0] ^= 1,
                    2 => other.attributes.ccsid = Some(1047),
                    3 => {
                        other.version += 1;
                        other.attributes_version += 1;
                        other.catalog.version += 1;
                    }
                    _ => other.catalog.related = Some(DatasetName::new(USER_DATASET, 128).unwrap()),
                }
                assert!(!transition_matches(Mutation::Add, &before, &bad, &trace));
            }
            let mut bad = after.clone();
            bad.0.remove(name);
            assert!(!transition_matches(Mutation::Add, &before, &bad, &trace));
        }
        let mut bad = after.clone();
        let mut extra = bad.0[USER_DATASET].clone();
        let name = "AWS.M2.CARDDEMO.EXTRA";
        extra.catalog.name = DatasetName::new(name, 128).unwrap();
        bad.0.insert(name.into(), extra);
        assert!(!transition_matches(Mutation::Add, &before, &bad, &trace));
        assert!(require_unchanged(&before, &before).is_ok());
        assert!(require_unchanged(&before, &after).is_err());
    }

    #[test]
    fn lifecycle_rejects_wrong_missing_and_extra_mutation_effects() {
        let before = snapshot(Phase::Seed, 3);
        let after = snapshot(Phase::Added, 4);
        let good = traces(&mutation_effects(Mutation::Add));
        assert!(!transition_matches(Mutation::Add, &before, &after, &[]));
        for alter in 0..6 {
            let mut bad = good.clone();
            match alter {
                0 => bad[0].operation = CicsOperation::Rewrite,
                1 => bad[0].outcome = "DUPREC".into(),
                2 => bad[0].response = 14,
                3 => bad[0].response2 = 80,
                4 => bad.push(traces(&[normal(CicsOperation::Delete, 0)]).remove(0)),
                _ => bad[0].payload_bytes = 1,
            }
            assert!(!transition_matches(Mutation::Add, &before, &after, &bad));
        }
    }

    fn page_fields(page: Page, cp037: bool) -> BTreeMap<String, Vec<u8>> {
        let mut fields = BTreeMap::new();
        for (slot, selected) in LIST_ROWS.iter().enumerate().take(10) {
            let row = match page {
                Page::Second if slot == 0 => &LIST_ROWS[10],
                Page::Second => &BLANK_ROW,
                _ => selected,
            };
            for (column, prefix) in ["USRID", "FNAME", "LNAME", "UTYPE"].into_iter().enumerate() {
                let value = if cp037 { row[column].1 } else { row[column].0 };
                fields.insert(format!("{prefix}{:02}", slot + 1), value.to_vec());
            }
        }
        let page = if matches!(page, Page::Second) { 2 } else { 1 };
        let mut value = vec![if cp037 { 0xf0 } else { b'0' }; 8];
        value[7] += page;
        fields.insert("PAGENUM".into(), value);
        fields
    }

    fn page_response(fields: &BTreeMap<String, Vec<u8>>) -> serde_json::Value {
        let mut bytes = Vec::new();
        for (name, value) in fields {
            bytes.extend((name.len() as u32).to_be_bytes());
            bytes.extend(name.as_bytes());
            bytes.extend((value.len() as u32).to_be_bytes());
            bytes.extend(value);
        }
        serde_json::json!({"mapset":"COUSR00", "map":"COUSR0A", "screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
    }

    #[test]
    fn user_pages_reject_every_bad_byte_missing_extra_and_wrong_page_row() {
        for page in [Page::First, Page::Second, Page::Restored] {
            for cp037 in [false, true] {
                let good = page_fields(page, cp037);
                assert!(list_fields_match(&good, page));
                for (name, value) in &good {
                    for at in 0..value.len() {
                        let mut bad = good.clone();
                        bad.get_mut(name).unwrap()[at] ^= 1;
                        assert!(!list_fields_match(&bad, page), "{name} byte={at}");
                    }
                    let mut bad = good.clone();
                    bad.remove(name);
                    assert!(!list_fields_match(&bad, page));
                }
                let mut bad = good.clone();
                bad.insert("USRID11".into(), b"EXTRA001".to_vec());
                assert!(!list_fields_match(&bad, page));
                let mut bad = good.clone();
                bad.insert("USRID01".into(), LIST_ROWS[9][0].0.to_vec());
                assert!(!list_fields_match(&bad, page));
                if matches!(page, Page::Second) {
                    let mut bad = good.clone();
                    bad.insert("USRID02".into(), LIST_ROWS[0][0].0.to_vec());
                    assert!(!list_fields_match(&bad, page));
                }
            }
        }
    }

    #[test]
    fn user_browse_requires_actual_cursor_direction_end_and_unchanged_full_state() {
        let snapshot = snapshot(Phase::Added, 4);
        for page in [Page::First, Page::Second, Page::Restored] {
            let response = page_response(&page_fields(page, false));
            let trace = traces(&page_effects(page));
            assert!(page_matches(page, &response, &trace, &snapshot, &snapshot).unwrap());
            let mut wrong_map = response.clone();
            wrong_map["map"] = "COUSR2A".into();
            assert!(!page_matches(page, &wrong_map, &trace, &snapshot, &snapshot).unwrap());
            for at in 0..trace.len() {
                let mut bad = trace.clone();
                bad.remove(at);
                assert!(!page_matches(page, &response, &bad, &snapshot, &snapshot).unwrap());
            }
            let mut bad = trace.clone();
            bad[1].operation = if matches!(page, Page::Restored) {
                CicsOperation::ReadNext
            } else {
                CicsOperation::ReadPrev
            };
            assert!(!page_matches(page, &response, &bad, &snapshot, &snapshot).unwrap());
            let mut bad = trace.clone();
            bad.push(traces(&[normal(CicsOperation::Write, 0)]).remove(0));
            assert!(!page_matches(page, &response, &bad, &snapshot, &snapshot).unwrap());
            let mut changed = snapshot.clone();
            changed.0.get_mut(DATASETS[0]).unwrap().records[0][0] ^= 1;
            assert!(!page_matches(page, &response, &trace, &snapshot, &changed).unwrap());
        }
    }
}
