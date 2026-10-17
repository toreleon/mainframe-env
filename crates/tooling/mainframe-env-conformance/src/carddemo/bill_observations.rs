//! Exact source-owned bill delta and actual CICS time in this UTC fixture.
//! No commit-and-rollback, generic package, AIX-browse or IBM parity token.
use super::insert_observations as inserted;
use super::*;
use inserted::{Rows, State};
use mainframe_env_cics::CicsTraceEntry;
use mainframe_env_host_api::CatalogEntryKind;
use std::time::{SystemTime, UNIX_EPOCH};

const ACCOUNT: &str = "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS";
const KEY_TWO: &[u8; 16] = &[
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf2,
];
const SOURCES: &[(&str, &str)] = &[
    (
        "app/cbl/COBIL00C.cbl",
        "b5c46039eb8fd2f7ed7d3ca379172e56fa87385a369023bef706b04986fe1cfb",
    ),
    (
        "app/cpy/CVTRA05Y.cpy",
        "d7bde0e78ff608497087b9909c889ed39e347269f964bda767b92b547fbb5fec",
    ),
    (
        "app/cpy/CVACT01Y.cpy",
        "81a08bad15af5664326a6f0af3650f570821c4857ffdec3a6a39f91f07dca728",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy/CSDAT01Y.cpy",
        "61dd7b01d974c13cd47f71774c613dd67996418a22fae730d09b246c0b710ad7",
    ),
    (
        "app/cpy-bms/COBIL00.CPY",
        "a8d1a371182c73b41b4effd2d2a9c3b005f503cb0c8e88f5b2f5b5062e739e85",
    ),
    (
        "app/bms/COBIL00.bms",
        "f2101ff2143cd4746a5ffe0c4eeec977dfdad8410dee19236e0bdfb5ad58657f",
    ),
    (
        "app/csd/CARDDEMO.CSD",
        "28ded93b49e188ae0100d6a964bd921a4de33414662e670b28f19bee41917cdb",
    ),
    (
        "app/jcl/ACCTFILE.jcl",
        "febba716137a3f59302ef1136c60dccce4d07d32e1d118b15d48f99266dea9e7",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCDATA.PS",
        "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCTDATA.PS",
        "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS",
        "b07ab2e5fa2500501c1dcaffe478ee74c4562a290174020a093dfa6f0e6325d3",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
        "aef36e51163a80999656487416e9edeec70b1e190fc70185c8e86c577e135073",
    ),
];

const ACCOUNT_BEFORE: &[u8; 300] = &[
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf5, 0xf0, 0xe8, 0xf0, 0xf0, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf4, 0xf9, 0xf2, 0xf0, 0xc0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf6, 0xf1,
    0xf6, 0xf9, 0xf0, 0xc0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf4, 0xf5, 0xf8, 0xf7, 0xf0, 0xc0,
    0xf2, 0xf0, 0xf1, 0xf1, 0x60, 0xf0, 0xf4, 0x60, 0xf2, 0xf2, 0xf2, 0xf0, 0xf2, 0xf3, 0x60, 0xf0,
    0xf3, 0x60, 0xf0, 0xf9, 0xf2, 0xf0, 0xf2, 0xf3, 0x60, 0xf0, 0xf3, 0x60, 0xf0, 0xf9, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xc0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xc0, 0xc1, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
];

const ACCOUNT_PAID: &[u8; 300] = &[
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf5, 0xf0, 0xe8, 0xf0, 0xf0, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xc0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf6, 0xf1,
    0xf6, 0xf9, 0xf0, 0xc0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf4, 0xf5, 0xf8, 0xf7, 0xf0, 0xc0,
    0xf2, 0xf0, 0xf1, 0xf1, 0x60, 0xf0, 0xf4, 0x60, 0xf2, 0xf2, 0xf2, 0xf0, 0xf2, 0xf3, 0x60, 0xf0,
    0xf3, 0x60, 0xf0, 0xf9, 0xf2, 0xf0, 0xf2, 0xf3, 0x60, 0xf0, 0xf3, 0x60, 0xf0, 0xf9, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xc0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xc0, 0xc1, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
];

const PAYMENT_STATIC: &[u8; 350] = &[
    0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf2,
    0xf0, 0xf2, 0xf0, 0xf0, 0xf0, 0xf2, 0xd7, 0xd6, 0xe2, 0x40, 0xe3, 0xc5, 0xd9, 0xd4, 0x40, 0x40,
    0xc2, 0xc9, 0xd3, 0xd3, 0x40, 0xd7, 0xc1, 0xe8, 0xd4, 0xc5, 0xd5, 0xe3, 0x40, 0x60, 0x40, 0xd6,
    0xd5, 0xd3, 0xc9, 0xd5, 0xc5, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf4, 0xf9, 0xf2, 0xf0, 0xc0, 0xf9,
    0xf9, 0xf9, 0xf9, 0xf9, 0xf9, 0xf9, 0xf9, 0xf9, 0xc2, 0xc9, 0xd3, 0xd3, 0x40, 0xd7, 0xc1, 0xe8,
    0xd4, 0xc5, 0xd5, 0xe3, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xd5, 0x61, 0xc1, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xd5, 0x61, 0xc1, 0x40,
    0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0xf0, 0xf5, 0xf0, 0xf0, 0xf0, 0xf2, 0xf4, 0xf4, 0xf5, 0xf3,
    0xf7, 0xf6, 0xf5, 0xf7, 0xf4, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xf0, 0xf0, 0xf0, 0xf0, 0xf1, 0xf0, 0xf0,
];

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.online.bill_bytes_clock_drift", detail)
}
fn refuse(condition: bool, detail: &str) -> Result<(), CorpusProblem> {
    if condition {
        Err(drift(detail))
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
pub(super) fn utc_second() -> Result<u64, CorpusProblem> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| drift(error.to_string()))
}
fn row<'a>(state: &'a State, name: &str) -> Result<&'a Rows, CorpusProblem> {
    state
        .get(name)
        .ok_or_else(|| drift(format!("missing selected dataset {name}")))
}
fn selected_account(accounts: &Rows) -> Result<usize, CorpusProblem> {
    let positions = accounts
        .identities
        .iter()
        .enumerate()
        .filter(|(_, key)| key.as_slice() == &ACCOUNT_BEFORE[..11])
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    refuse(positions.len() != 1, "account 50 is absent or duplicated")?;
    Ok(positions[0])
}
fn validate_before(before: &State) -> Result<(), CorpusProblem> {
    refuse(
        !inserted::names_match(before),
        "complete selected inventory differs",
    )?;
    for (name, index) in [(inserted::PRIMARY, false), (inserted::LOCAL_INDEX, true)] {
        let actual = row(before, name)?;
        refuse(
            actual.records != vec![inserted::BOOTSTRAP.to_vec(), inserted::PURCHASE.to_vec()]
                || actual.identities != vec![vec![0; 16], inserted::PURCHASE[..16].to_vec()]
                || actual.attributes != inserted::attributes(index)
                || actual.version != 2
                || actual.attributes_version != 2
                || actual.catalog.version != 2
                || actual.catalog.name.as_str() != name
                || actual.catalog.kind
                    != if index {
                        CatalogEntryKind::AlternateIndex
                    } else {
                        CatalogEntryKind::Dataset
                    }
                || actual.catalog.related.as_ref().map(DatasetName::as_str)
                    != if index { Some(inserted::PRIMARY) } else { None },
            "before-payment primary/local-index insertion state differs",
        )?;
    }
    let accounts = row(before, ACCOUNT)?;
    let selected = selected_account(accounts)?;
    let attributes = DatasetAttributes {
        organization: DatasetOrganization::KeySequenced,
        record_format: RecordFormat::Fixed,
        logical_record_length: 300,
        key_offset: Some(0),
        key_length: Some(11),
        ccsid: Some(37),
    };
    refuse(
        accounts.records.len() != 50
            || accounts.identities.len() != 50
            || accounts.records[selected].as_slice() != ACCOUNT_BEFORE
            || accounts.attributes != attributes
            || accounts.version == 0
            || accounts.attributes_version != accounts.version
            || accounts.catalog.version != accounts.version
            || accounts.catalog.name.as_str() != ACCOUNT
            || accounts.catalog.kind != CatalogEntryKind::Dataset
            || accounts.catalog.related.is_some()
            || accounts
                .records
                .iter()
                .zip(&accounts.identities)
                .any(|(bytes, key)| bytes.len() != 300 || key.as_slice() != &bytes[..11]),
        "before-payment pinned account 50, complete cardinality/keys/layout/version differ",
    )?;
    let xref = row(before, inserted::XREF)?;
    let selected = xref
        .records
        .iter()
        .position(|bytes| bytes.as_slice() == inserted::SELECTED_XREF)
        .ok_or_else(|| drift("before-payment pinned account/card XREF is missing"))?;
    refuse(
        xref.identities
            .get(selected)
            .is_none_or(|key| key.as_slice() != &inserted::SELECTED_XREF[..16]),
        "pinned XREF raw primary key differs",
    )
}
fn compare_metadata(before: &Rows, after: &Rows, version: u64) -> Result<(), CorpusProblem> {
    let mut catalog = before.catalog.clone();
    catalog.version = version;
    refuse(
        after.catalog != catalog
            || after.attributes != before.attributes
            || after.version != version
            || after.attributes_version != version,
        "affected dataset metadata/key layout or exact revision differs",
    )
}
fn digit(byte: u8) -> Result<u64, CorpusProblem> {
    match byte {
        0xf0..=0xf9 => Ok(u64::from(byte - 0xf0)),
        _ => Err(drift("timestamp digit is not Cp037 display numeric")),
    }
}
fn decimal(bytes: &[u8]) -> Result<u64, CorpusProblem> {
    bytes
        .iter()
        .try_fold(0, |value, byte| Ok(value * 10 + digit(*byte)?))
}
fn leap(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
// Independent Gregorian arithmetic. Never call the product clock formatter,
// COBOL formatter, screen values, or an inserted row to construct expected time.
fn timestamp_second(bytes: &[u8], lower: u64, upper: u64) -> Result<u64, CorpusProblem> {
    refuse(
        bytes.len() != 26 || lower > upper,
        "timestamp width or independent UTC bounds differ",
    )?;
    for (offset, value) in [
        (4, 0x60),
        (7, 0x60),
        (10, 0x40),
        (13, 0x7a),
        (16, 0x7a),
        (19, 0x4b),
    ] {
        refuse(
            bytes[offset] != value,
            "timestamp literal separator differs",
        )?;
    }
    refuse(
        bytes[20..] != [0xf0; 6],
        "timestamp microseconds are not all zero",
    )?;
    let year = decimal(&bytes[..4])?;
    let month = decimal(&bytes[5..7])?;
    let day = decimal(&bytes[8..10])?;
    let hour = decimal(&bytes[11..13])?;
    let minute = decimal(&bytes[14..16])?;
    let second = decimal(&bytes[17..19])?;
    refuse(
        !(1970..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || hour > 23
            || minute > 59
            || second > 59,
        "timestamp calendar or clock domain differs",
    )?;
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    refuse(
        day == 0 || day > months[(month - 1) as usize],
        "timestamp day is invalid for its Gregorian month",
    )?;
    let prior = year - 1;
    let days = 365 * prior + prior / 4 - prior / 100 + prior / 400 - 719_162
        + months[..(month - 1) as usize].iter().sum::<u64>()
        + day
        - 1;
    let epoch = days * 86_400 + hour * 3_600 + minute * 60 + second;
    refuse(
        epoch < lower || epoch > upper,
        "actual CICS timestamp is outside independently observed UTC seconds",
    )?;
    Ok(epoch)
}
fn compare_payment(bytes: &[u8], lower: u64, upper: u64) -> Result<(), CorpusProblem> {
    refuse(bytes.len() != 350, "payment record width differs")?;
    refuse(
        bytes[..278] != PAYMENT_STATIC[..278] || bytes[330..] != PAYMENT_STATIC[330..],
        "complete static payment fields, amount, key, padding or retained FILLER differ",
    )?;
    refuse(
        bytes[278..304] != bytes[304..330],
        "orig/proc timestamps differ",
    )?;
    timestamp_second(&bytes[278..304], lower, upper)?;
    Ok(())
}
fn compare_state(
    before: &State,
    after: &State,
    lower: u64,
    upper: u64,
) -> Result<(), CorpusProblem> {
    validate_before(before)?;
    refuse(
        !inserted::names_match(after),
        "payment added, lost or renamed a dataset/catalog entry",
    )?;
    let primary = row(after, inserted::PRIMARY)?;
    refuse(
        primary.records.len() != 3
            || primary.identities
                != vec![
                    vec![0; 16],
                    inserted::PURCHASE[..16].to_vec(),
                    KEY_TWO.to_vec(),
                ],
        "payment primary cardinality, ordering or independently fixed raw keys differ",
    )?;
    refuse(
        primary.records[..2] != [inserted::BOOTSTRAP.to_vec(), inserted::PURCHASE.to_vec()],
        "payment changed either original primary transaction",
    )?;
    compare_payment(&primary.records[2], lower, upper)?;
    compare_metadata(row(before, inserted::PRIMARY)?, primary, 3)?;
    let index = row(after, inserted::LOCAL_INDEX)?;
    compare_metadata(row(before, inserted::LOCAL_INDEX)?, index, 3)?;
    // This declared local view is sorted by its actual fixed secondary layout.
    // Each candidate row has already been validated completely above. This
    // coherence comparison does not establish an application AIX browse.
    let mut ordered = primary.records.clone();
    ordered.sort_by(|left, right| {
        left[304..330]
            .cmp(&right[304..330])
            .then(left[..16].cmp(&right[..16]))
    });
    let keys = ordered
        .iter()
        .map(|bytes| bytes[..16].to_vec())
        .collect::<Vec<_>>();
    refuse(
        index.records != ordered || index.identities != keys,
        "complete declared local-index rows/identities/order differ",
    )?;
    let old_accounts = row(before, ACCOUNT)?;
    let accounts = row(after, ACCOUNT)?;
    let version = old_accounts
        .version
        .checked_add(1)
        .ok_or_else(|| drift("account revision overflow"))?;
    compare_metadata(old_accounts, accounts, version)?;
    let mut expected = old_accounts.records.clone();
    expected[selected_account(old_accounts)?] = ACCOUNT_PAID.to_vec();
    refuse(
        accounts.records != expected || accounts.identities != old_accounts.identities,
        "complete paid account bytes/key/order/cardinality or other account rows differ",
    )?;
    for name in inserted::NAMES {
        if ![ACCOUNT, inserted::PRIMARY, inserted::LOCAL_INDEX].contains(name) {
            refuse(
                row(before, name)? != row(after, name)?,
                "payment changed an unrelated dataset record/key/layout/catalog/version",
            )?;
        }
    }
    Ok(())
}
fn compare_trace(trace: &[CicsTraceEntry]) -> Result<(), CorpusProblem> {
    let actual = trace
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
                    | CicsOperation::Asktime
                    | CicsOperation::AsktimeEib
                    | CicsOperation::FormatTime
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
    let expected = [
        (CicsOperation::Read, 300),
        (CicsOperation::Read, 50),
        (CicsOperation::StartBrowse, 0),
        (CicsOperation::ReadPrev, 350),
        (CicsOperation::EndBrowse, 0),
        (CicsOperation::Asktime, 0),
        (CicsOperation::FormatTime, 0),
        (CicsOperation::Write, 0),
        (CicsOperation::Rewrite, 0),
    ]
    .into_iter()
    .map(|(operation, bytes)| (operation, "NORMAL", 0, 0, bytes))
    .collect::<Vec<_>>();
    refuse(
        actual != expected,
        "actual bill file/time ordering, conditions, RESP/RESP2 or reply widths differ",
    )
}
fn compare_screen(response: &serde_json::Value) -> Result<(), CorpusProblem> {
    refuse(
        response["mapset"] != "COBIL00" || response["map"] != "COBIL0A",
        "payment reached wrong selected map",
    )?;
    let fields = online_screen_fields(response)?;
    for (name, ascii, cp037) in [
        (
            "PGMNAME",
            b"COBIL00C".as_slice(),
            &[0xc3, 0xd6, 0xc2, 0xc9, 0xd3, 0xf0, 0xf0, 0xc3][..],
        ),
        ("TRNNAME", b"CB00".as_slice(), &[0xc3, 0xc2, 0xf0, 0xf0][..]),
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
pub(super) struct BillProof {
    before: State,
    trace_start: usize,
    writes_before: usize,
    rewrites_before: usize,
}
pub(super) struct ClockProof {
    _verified: (),
}
impl BillProof {
    pub(super) fn capture(
        server: &ProductServer,
        corpus: &Path,
        session: &str,
    ) -> Result<Self, CorpusProblem> {
        verify_sources(corpus)?;
        let before = inserted::capture_state(server)?;
        validate_before(&before)?;
        Ok(Self {
            before,
            trace_start: server
                .online_trace(session)
                .map_err(terminal_problem)?
                .len(),
            writes_before: server
                .online_operation_count(CicsOperation::Write)
                .map_err(terminal_problem)?,
            rewrites_before: server
                .online_operation_count(CicsOperation::Rewrite)
                .map_err(terminal_problem)?,
        })
    }
    pub(super) fn compare(
        self,
        server: &ProductServer,
        session: &str,
        response: &serde_json::Value,
        lower: u64,
        upper: u64,
    ) -> Result<ClockProof, CorpusProblem> {
        let after = inserted::capture_state(server)?;
        compare_state(&self.before, &after, lower, upper)?;
        let trace = server.online_trace(session).map_err(terminal_problem)?;
        let delta = trace
            .get(self.trace_start..)
            .ok_or_else(|| drift("bill trace shrank"))?;
        compare_trace(delta)?;
        compare_screen(response)?;
        refuse(
            self.writes_before.checked_add(1)
                != Some(
                    server
                        .online_operation_count(CicsOperation::Write)
                        .map_err(terminal_problem)?,
                )
                || self.rewrites_before.checked_add(1)
                    != Some(
                        server
                            .online_operation_count(CicsOperation::Rewrite)
                            .map_err(terminal_problem)?,
                    ),
            "actual WRITE and REWRITE counters did not each advance exactly one",
        )?;
        Ok(ClockProof { _verified: () })
    }
}
impl ClockProof {
    // The actual witness is constructed only after full state, effects and UTC
    // comparisons. The original bill predicate must also pass before this bind.
    pub(super) fn bind(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J07",
            "ASKTIME and FORMATTIME",
            false,
            || Ok(drift("actual bill clock proof refused")),
        )
    }
}

#[cfg(test)]
#[path = "bill_observations_tests.rs"]
mod tests;
