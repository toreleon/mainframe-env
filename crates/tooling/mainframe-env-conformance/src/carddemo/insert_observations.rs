//! Full raw dataset delta for the actual base COTRN02 primary-KSDS insert.
//! Local installed-index coherence earns no application AIX-browse token.
use super::*;
use mainframe_env_cics::CicsTraceEntry;
use mainframe_env_host_api::{CatalogEntryKind, CatalogListEntry};

pub(super) const PRIMARY: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS";
pub(super) const LOCAL_INDEX: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH";
pub(super) const XREF: &str = "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS";

pub(super) const NAMES: &[&str] = &[
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
    "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
];

const SOURCES: &[(&str, &str)] = &[
    (
        "app/cbl/COTRN02C.cbl",
        "dc33b4c797a98d2732070a223f43cc78992c2846681163a032e376630742b1ed",
    ),
    (
        "app/cbl/CSUTLDTC.cbl",
        "58c165dcfc3927232c02c152c673e3dee551587b85e7fe56ca67325c980eebb6",
    ),
    (
        "app/cpy/CVTRA05Y.cpy",
        "d7bde0e78ff608497087b9909c889ed39e347269f964bda767b92b547fbb5fec",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy-bms/COTRN02.CPY",
        "1eae5b624b8e7f97c24e94a373065d5c648340e21019bd7736dc663ab0d8642d",
    ),
    (
        "app/bms/COTRN02.bms",
        "8b46b5ae879119b9d8395b712970ebc5b8d5043efd351a47f9a31a451e073042",
    ),
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/jcl/TRANFILE.jcl",
        "360080ae5bf5153c3ecbf82385b070a6c7a2cdbf18870685951bf9894516730a",
    ),
    (
        "app/jcl/TRANIDX.jcl",
        "64a06b44c816ee666a0e82e24fbf040d5ed792d158c93a0ac477db5dfa30dcae",
    ),
    (
        "app/jcl/XREFFILE.jcl",
        "ae0edd90056cccacd26412fad2cf3d95656c2a2810b66d8c316cbaac9e81248f",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
        "aef36e51163a80999656487416e9edeec70b1e190fc70185c8e86c577e135073",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS",
        "b07ab2e5fa2500501c1dcaffe478ee74c4562a290174020a093dfa6f0e6325d3",
    ),
];

// Frozen from pinned CVTRA05Y, controlled input literals and source seed.
// Existing local decode_display converts NUL key digits to 0. INITIALIZE without
// WITH FILLER retains the source bootstrap tail. Neither grants IBM semantics.
pub(super) const BOOTSTRAP: &[u8; 350] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf1, 0xf0, 0xf0,
];

pub(super) const PURCHASE: &[u8; 350] = &[
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf1,
    0xf0, 0xf1, 0xf0, 0xf0, 0xf0, 0xf1, 0xd6, 0xd5, 0xd3, 0xc9, 0xd5, 0xc5, 0x40, 0x40, 0x40, 0x40,
    0xc3, 0xc5, 0xd9, 0xe3, 0xc9, 0xc6, 0xc9, 0xc5, 0xc4, 0x40, 0xd7, 0xe4, 0xd9, 0xc3, 0xc8, 0xc1,
    0xe2, 0xc5, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf1, 0xf0, 0xc0, 0xf1,
    0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xc3, 0xc5, 0xd9, 0xe3, 0xc9, 0xc6, 0xc9, 0xc5,
    0xc4, 0x40, 0xe2, 0xc8, 0xd6, 0xd7, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xc8, 0xc1, 0xd5, 0xd6, 0xc9, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xf7, 0xf0, 0xf0, 0xf0,
    0xf0, 0x40, 0x40, 0x40, 0x40, 0x40, 0xf0, 0xf5, 0xf0, 0xf0, 0xf0, 0xf2, 0xf4, 0xf4, 0xf5, 0xf3,
    0xf7, 0xf6, 0xf5, 0xf7, 0xf4, 0xf0, 0xf2, 0xf0, 0xf2, 0xf6, 0x60, 0xf0, 0xf8, 0x60, 0xf3, 0xf0,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0xf2, 0xf0, 0xf2, 0xf6, 0x60, 0xf0, 0xf8, 0x60, 0xf3, 0xf0, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf1, 0xf0, 0xf0,
];

pub(super) const SELECTED_XREF: &[u8; 50] = &[
    0xf0, 0xf5, 0xf0, 0xf0, 0xf0, 0xf2, 0xf4, 0xf4, 0xf5, 0xf3, 0xf7, 0xf6, 0xf5, 0xf7, 0xf4, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf5, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf5, 0xf0, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40,
];

const PROGRAM_ASCII: &[u8] = b"COTRN02C";
const PROGRAM_CP037: &[u8; 8] = &[0xc3, 0xd6, 0xe3, 0xd9, 0xd5, 0xf0, 0xf2, 0xc3];

const TRANSACTION_ASCII: &[u8] = b"CT02";
const TRANSACTION_CP037: &[u8; 4] = &[0xc3, 0xe3, 0xf0, 0xf2];

const MESSAGE_ASCII: &[u8] =
    b"Transaction added successfully.  Your Tran ID is 0000000000000001.            ";
const MESSAGE_CP037: &[u8; 78] = &[
    0xe3, 0x99, 0x81, 0x95, 0xa2, 0x81, 0x83, 0xa3, 0x89, 0x96, 0x95, 0x40, 0x81, 0x84, 0x84, 0x85,
    0x84, 0x40, 0xa2, 0xa4, 0x83, 0x83, 0x85, 0xa2, 0xa2, 0x86, 0xa4, 0x93, 0x93, 0xa8, 0x4b, 0x40,
    0x40, 0xe8, 0x96, 0xa4, 0x99, 0x40, 0xe3, 0x99, 0x81, 0x95, 0x40, 0xc9, 0xc4, 0x40, 0x89, 0xa2,
    0x40, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
    0xf1, 0x4b, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
];

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Rows {
    pub(super) catalog: CatalogListEntry,
    pub(super) attributes: DatasetAttributes,
    pub(super) attributes_version: u64,
    pub(super) records: Vec<Vec<u8>>,
    pub(super) identities: Vec<Vec<u8>>,
    pub(super) version: u64,
}
pub(super) type State = BTreeMap<String, Rows>;

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.transaction_insert_bytes_drift", detail)
}
fn refuse(condition: bool, detail: &str) -> Result<(), CorpusProblem> {
    if condition {
        Err(drift(detail))
    } else {
        Ok(())
    }
}
fn source_identity(relative: &str, bytes: &[u8]) -> Result<(), CorpusProblem> {
    let expected = SOURCES
        .iter()
        .find(|(path, _)| *path == relative)
        .ok_or_else(|| drift("unowned source identity"))?
        .1;
    refuse(format!("{:x}", Sha256::digest(bytes)) != expected, relative)
}
fn verify_sources(corpus: &Path) -> Result<(), CorpusProblem> {
    for (relative, _) in SOURCES {
        source_identity(relative, &read_corpus_file(corpus, &corpus.join(relative))?)?;
    }
    Ok(())
}
pub(super) fn names_match(state: &State) -> bool {
    state.keys().map(String::as_str).eq(NAMES.iter().copied())
}
fn rows<'a>(state: &'a State, name: &str) -> Result<&'a Rows, CorpusProblem> {
    state
        .get(name)
        .ok_or_else(|| drift(format!("missing selected dataset {name}")))
}
pub(super) fn attributes(index: bool) -> DatasetAttributes {
    DatasetAttributes {
        organization: DatasetOrganization::KeySequenced,
        record_format: RecordFormat::Fixed,
        logical_record_length: 350,
        key_offset: Some(if index { 304 } else { 0 }),
        key_length: Some(if index { 26 } else { 16 }),
        ccsid: Some(37),
    }
}
fn validate_before(before: &State) -> Result<(), CorpusProblem> {
    refuse(
        !names_match(before),
        "complete selected dataset inventory differs",
    )?;
    for (name, index) in [(PRIMARY, false), (LOCAL_INDEX, true)] {
        let actual = rows(before, name)?;
        refuse(
            actual.records != vec![BOOTSTRAP.to_vec()]
                || actual.identities != vec![vec![0; 16]]
                || actual.attributes != attributes(index)
                || actual.version != 1
                || actual.attributes_version != 1
                || actual.catalog.version != 1
                || actual.catalog.name.as_str() != name
                || actual.catalog.kind
                    != if index {
                        CatalogEntryKind::AlternateIndex
                    } else {
                        CatalogEntryKind::Dataset
                    }
                || actual.catalog.related.as_ref().map(DatasetName::as_str)
                    != if index { Some(PRIMARY) } else { None },
            "primary/bootstrap or declared local index records/keys/layout/version differ",
        )?;
    }
    let xref = rows(before, XREF)?;
    let selected = xref
        .records
        .iter()
        .position(|record| record.as_slice() == SELECTED_XREF)
        .ok_or_else(|| {
            drift("actual XREF lacks the independently pinned account 50/card record")
        })?;
    refuse(
        xref.identities
            .get(selected)
            .is_none_or(|key| key.as_slice() != &SELECTED_XREF[..16]),
        "selected XREF source record has the wrong raw primary key",
    )
}
fn expected_insert(original: &Rows) -> Rows {
    let mut expected = original.clone();
    // Both primary and this declared local view order the NUL bootstrap first.
    // The primary identity is the first 16 bytes in both result contracts.
    expected.records = vec![BOOTSTRAP.to_vec(), PURCHASE.to_vec()];
    expected.identities = vec![vec![0; 16], PURCHASE[..16].to_vec()];
    expected.version = 2;
    expected.attributes_version = 2;
    expected.catalog.version = 2;
    expected
}
fn compare_state(before: &State, after: &State) -> Result<(), CorpusProblem> {
    validate_before(before)?;
    refuse(
        !names_match(after),
        "insert added, lost or renamed a dataset/catalog entry",
    )?;
    for name in NAMES {
        let expected = rows(before, name)?;
        let actual = rows(after, name)?;
        if *name == PRIMARY || *name == LOCAL_INDEX {
            refuse(
                actual != &expected_insert(expected),
                "complete 350-byte primary/local-index delta differs",
            )?;
        } else {
            refuse(
                actual != expected,
                "insert changed an unrelated dataset record/key/layout/catalog/version",
            )?;
        }
    }
    Ok(())
}
fn compare_trace(trace: &[CicsTraceEntry]) -> Result<(), CorpusProblem> {
    let file = trace
        .iter()
        .filter(|entry| {
            matches!(
                entry.operation,
                CicsOperation::Read
                    | CicsOperation::ReadNext
                    | CicsOperation::ReadPrev
                    | CicsOperation::StartBrowse
                    | CicsOperation::ResetBrowse
                    | CicsOperation::EndBrowse
                    | CicsOperation::Write
                    | CicsOperation::Rewrite
                    | CicsOperation::Delete
            )
        })
        .map(|entry| {
            (
                entry.operation,
                entry.outcome.as_str(),
                entry.response,
                entry.response2,
                entry.payload_bytes,
            )
        })
        .collect::<Vec<_>>();
    refuse(
        file != vec![
            (CicsOperation::Read, "NORMAL", 0, 0, 50),
            (CicsOperation::StartBrowse, "NORMAL", 0, 0, 0),
            (CicsOperation::ReadPrev, "NORMAL", 0, 0, 350),
            (CicsOperation::EndBrowse, "NORMAL", 0, 0, 0),
            (CicsOperation::Write, "NORMAL", 0, 0, 0),
        ],
        "actual insert file sequence/condition/RESP/RESP2/payload differs",
    )
}
fn compare_screen(response: &serde_json::Value) -> Result<(), CorpusProblem> {
    refuse(
        response["mapset"] != "COTRN02" || response["map"] != "COTRN2A",
        "insert reached wrong selected map",
    )?;
    let fields = online_screen_fields(response)?;
    for (name, ascii, cp037) in [
        ("PGMNAME", PROGRAM_ASCII, PROGRAM_CP037.as_slice()),
        ("TRNNAME", TRANSACTION_ASCII, TRANSACTION_CP037.as_slice()),
        ("ERRMSG", MESSAGE_ASCII, MESSAGE_CP037.as_slice()),
    ] {
        refuse(
            fields
                .get(name)
                .is_none_or(|value| value.as_slice() != ascii && value.as_slice() != cp037),
            name,
        )?;
    }
    Ok(())
}
pub(super) fn capture_state(server: &ProductServer) -> Result<State, CorpusProblem> {
    let catalog = match server
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
        } => entries,
        _ => {
            return Err(drift(
                "bounded complete dataset catalog is missing or truncated",
            ));
        }
    };
    let mut state = State::new();
    for catalog in catalog {
        let dataset = catalog.name.clone();
        let name = dataset.as_str().to_string();
        let (attributes, attributes_version) = match server
            .dataset_service()
            .invoke(DatasetRequest::Attributes {
                dataset: dataset.clone(),
            })
            .map_err(terminal_problem)?
        {
            DatasetResult::Attributes {
                attributes,
                version,
            } => (attributes, version),
            _ => return Err(drift("selected dataset attributes missing")),
        };
        // Largest admitted seed is 500 rows. 501 catches overflow instead of
        // treating a provider read ceiling as a complete result. No limit changes.
        let (records, identities, version) = match server
            .dataset_service()
            .invoke(DatasetRequest::Read {
                dataset,
                member: None,
                key: None,
                max_records: 501,
                control: Default::default(),
            })
            .map_err(terminal_problem)?
        {
            DatasetResult::Records {
                records,
                identities,
                version,
            } => (records, identities, version),
            _ => return Err(drift("selected raw records missing")),
        };
        refuse(
            records.len() >= 501
                || records.len() != identities.len()
                || version == 0
                || attributes_version != version
                || catalog.version != version,
            "raw dataset result is truncated or has inconsistent keys/versions",
        )?;
        let record = Rows {
            catalog,
            attributes,
            attributes_version,
            records,
            identities,
            version,
        };
        refuse(
            state.insert(name, record).is_some(),
            "duplicate dataset catalog identity",
        )?;
    }
    refuse(
        !names_match(&state),
        "complete 15-entry selected dataset catalog differs",
    )?;
    Ok(state)
}

pub(super) struct InsertProof {
    before: State,
    trace_start: usize,
    writes_before: usize,
}
impl InsertProof {
    pub(super) fn capture(
        server: &ProductServer,
        corpus: &Path,
        session: &str,
    ) -> Result<Self, CorpusProblem> {
        verify_sources(corpus)?;
        let before = capture_state(server)?;
        validate_before(&before)?;
        let trace_start = server
            .online_trace(session)
            .map_err(terminal_problem)?
            .len();
        let writes_before = server
            .online_operation_count(CicsOperation::Write)
            .map_err(terminal_problem)?;
        Ok(Self {
            before,
            trace_start,
            writes_before,
        })
    }
    pub(super) fn compare(
        self,
        server: &ProductServer,
        session: &str,
        response: &serde_json::Value,
    ) -> Result<(), CorpusProblem> {
        let after = capture_state(server)?;
        compare_state(&self.before, &after)?;
        let trace = server.online_trace(session).map_err(terminal_problem)?;
        let observed = trace
            .get(self.trace_start..)
            .ok_or_else(|| drift("insert trace shrank"))?;
        compare_trace(observed)?;
        compare_screen(response)?;
        let writes_after = server
            .online_operation_count(CicsOperation::Write)
            .map_err(terminal_problem)?;
        refuse(
            self.writes_before.checked_add(1) != Some(writes_after),
            "actual WRITE counter did not advance exactly 1",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        let mut before = State::new();
        for name in NAMES {
            let index = *name == LOCAL_INDEX;
            let core = *name == PRIMARY || index;
            before.insert(
                (*name).into(),
                Rows {
                    catalog: CatalogListEntry {
                        name: DatasetName::new(*name, 128).unwrap(),
                        kind: if index {
                            CatalogEntryKind::AlternateIndex
                        } else {
                            CatalogEntryKind::Dataset
                        },
                        related: index.then(|| DatasetName::new(PRIMARY, 128).unwrap()),
                        version: 1,
                    },
                    attributes: attributes(index),
                    attributes_version: 1,
                    records: if core {
                        vec![BOOTSTRAP.to_vec()]
                    } else if *name == XREF {
                        vec![SELECTED_XREF.to_vec()]
                    } else {
                        vec![b"unchanged".to_vec()]
                    },
                    identities: if core {
                        vec![vec![0; 16]]
                    } else if *name == XREF {
                        vec![SELECTED_XREF[..16].to_vec()]
                    } else {
                        vec![b"fixed".to_vec()]
                    },
                    version: 1,
                },
            );
        }
        before
    }
    fn inserted(before: &State) -> State {
        let mut after = before.clone();
        for name in [PRIMARY, LOCAL_INDEX] {
            after.insert(name.into(), expected_insert(&before[name]));
        }
        after
    }
    fn trace() -> Vec<CicsTraceEntry> {
        [
            (CicsOperation::Read, 50),
            (CicsOperation::StartBrowse, 0),
            (CicsOperation::ReadPrev, 350),
            (CicsOperation::EndBrowse, 0),
            (CicsOperation::Write, 0),
        ]
        .into_iter()
        .map(|(operation, payload_bytes)| CicsTraceEntry {
            operation,
            outcome: "NORMAL".into(),
            response: 0,
            response2: 0,
            payload_bytes,
        })
        .collect()
    }
    fn screen() -> serde_json::Value {
        let mut bytes = Vec::new();
        for (name, value) in [
            ("PGMNAME", PROGRAM_ASCII),
            ("TRNNAME", TRANSACTION_ASCII),
            ("ERRMSG", MESSAGE_ASCII),
        ] {
            bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(value);
        }
        serde_json::json!({"mapset":"COTRN02","map":"COTRN2A","screen_base64":base64::engine::general_purpose::STANDARD.encode(bytes)})
    }

    #[test]
    fn every_new_record_byte_including_filler_and_unchanged_bootstrap_is_owned() {
        let before = state();
        let correct = inserted(&before);
        assert!(compare_state(&before, &correct).is_ok());
        for offset in 0..350 {
            let mut after = correct.clone();
            after.get_mut(PRIMARY).unwrap().records[1][offset] ^= 1;
            assert!(compare_state(&before, &after).is_err(), "new byte{offset}");
            let mut after = correct.clone();
            after.get_mut(PRIMARY).unwrap().records[0][offset] ^= 1;
            assert!(
                compare_state(&before, &after).is_err(),
                "bootstrap byte{offset}"
            );
        }
    }
    #[test]
    fn wrong_cardinality_keys_order_versions_layout_or_unrelated_state_refuse() {
        let before = state();
        let correct = inserted(&before);
        for component in 0..10 {
            let mut after = correct.clone();
            let row = after.get_mut(PRIMARY).unwrap();
            match component {
                0 => {
                    row.records.pop();
                }
                1 => {
                    row.records.push(PURCHASE.to_vec());
                }
                2 => row.records.swap(0, 1),
                3 => row.identities[1][0] ^= 1,
                4 => {
                    let _ = row.identities.pop();
                }
                5 => row.version = 1,
                6 => row.attributes_version = 1,
                7 => row.catalog.version = 1,
                8 => row.attributes.key_offset = Some(1),
                _ => row.catalog.kind = CatalogEntryKind::Alias,
            }
            assert!(
                compare_state(&before, &after).is_err(),
                "component{component}"
            );
        }
        let mut after = correct.clone();
        after.remove(XREF);
        assert!(compare_state(&before, &after).is_err());
        for component in 0..4 {
            let mut after = correct.clone();
            let row = after.get_mut(XREF).unwrap();
            match component {
                0 => row.records[0][0] ^= 1,
                1 => row.identities[0][0] ^= 1,
                2 => row.version += 1,
                _ => row.catalog.related = Some(DatasetName::new(PRIMARY, 128).unwrap()),
            }
            assert!(compare_state(&before, &after).is_err());
        }
        let mut after = correct.clone();
        after.get_mut(LOCAL_INDEX).unwrap().records.swap(0, 1);
        assert!(compare_state(&before, &after).is_err());
        let mut after = correct.clone();
        after.get_mut(LOCAL_INDEX).unwrap().version = 1;
        assert!(compare_state(&before, &after).is_err());
    }
    #[test]
    fn insert_sequence_conditions_payload_and_success_identity_cannot_be_substituted() {
        let correct = trace();
        assert!(compare_trace(&correct).is_ok());
        for component in 0..5 {
            let mut actual = correct.clone();
            match component {
                0 => actual[4].outcome = "DUPREC".into(),
                1 => actual[4].response = 14,
                2 => actual[4].response2 = 80,
                3 => actual[2].payload_bytes = 349,
                _ => actual.swap(2, 3),
            }
            assert!(compare_trace(&actual).is_err());
        }
        let mut actual = correct.clone();
        actual.push(correct[4].clone());
        assert!(compare_trace(&actual).is_err());
        let mut actual = correct.clone();
        actual[4].operation = CicsOperation::Rewrite;
        assert!(compare_trace(&actual).is_err());
        let correct = screen();
        assert!(compare_screen(&correct).is_ok());
        let mut actual = correct.clone();
        actual["map"] = "COTRN1A".into();
        assert!(compare_screen(&actual).is_err());
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(correct["screen_base64"].as_str().unwrap())
            .unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        actual = correct.clone();
        actual["screen_base64"] = base64::engine::general_purpose::STANDARD
            .encode(bytes)
            .into();
        assert!(compare_screen(&actual).is_err());
    }
    #[test]
    fn changed_or_truncated_bootstrap_source_cannot_establish_expected_key() {
        let path = "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS.INIT";
        assert!(source_identity(path, BOOTSTRAP).is_ok());
        let mut wrong = BOOTSTRAP.to_vec();
        wrong[0] = 0xf0;
        assert!(source_identity(path, &wrong).is_err());
        let mut wrong = BOOTSTRAP.to_vec();
        wrong[349] ^= 1;
        assert!(source_identity(path, &wrong).is_err());
        assert!(source_identity(path, &BOOTSTRAP[..349]).is_err());
        let mut wrong = BOOTSTRAP.to_vec();
        wrong.push(0);
        assert!(source_identity(path, &wrong).is_err());
    }
}
