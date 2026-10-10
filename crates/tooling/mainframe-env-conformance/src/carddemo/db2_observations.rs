//! Independent first-party ordered PS/VSAM oracle; no IBM execution credit.
// CardDemo 59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e:
// app/app-transaction-type-db2/ctl/{DB2LTTYP,DB2LTCAT}.ctl,
// app/app-transaction-type-db2/jcl/TRANEXTR.jcl and app/jcl/{TRANTYPE,TRANCATG}.jcl.
// Controlled MNTTRDB2 updates type 02 to BATCH PAYMENT; retain source REVERAL.
// The existing NEW DD/IDCAMS routes select CCSID37. Literal Cp037 bytes and
// digit keys were frozen with Python's independent cp037 codec from those
// first-party fields, padding and suffixes, never the runtime SQL/byte encoder.
use super::journey_closure::AuthorityKind;
use super::{
    CorpusProblem, DatasetName, DatasetOrganization, DatasetRequest, DatasetResult, ProductServer,
    RecordFormat, RouteObservations, terminal_problem,
};

const EXPECTED_TYPES: [&[u8]; 7] = [
    b"\xf0\xf1\xd7\xe4\xd9\xc3\xc8\xc1\xe2\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
    b"\xf0\xf2\xc2\xc1\xe3\xc3\xc8\x40\xd7\xc1\xe8\xd4\xc5\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
    b"\xf0\xf3\xc3\xd9\xc5\xc4\xc9\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
    b"\xf0\xf4\xc1\xe4\xe3\xc8\xd6\xd9\xc9\xe9\xc1\xe3\xc9\xd6\xd5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
    b"\xf0\xf5\xd9\xc5\xc6\xe4\xd5\xc4\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
    b"\xf0\xf6\xd9\xc5\xe5\xc5\xd9\xc1\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
    b"\xf0\xf7\xc1\xc4\xd1\xe4\xe2\xe3\xd4\xc5\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0",
];

const EXPECTED_CATEGORIES: [&[u8]; 18] = [
    b"\xf0\xf1\xf0\xf0\xf0\xf1\xd9\xc5\xc7\xe4\xd3\xc1\xd9\x40\xe2\xc1\xd3\xc5\xe2\x40\xc4\xd9\xc1\xc6\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf1\xf0\xf0\xf0\xf2\xd9\xc5\xc7\xe4\xd3\xc1\xd9\x40\xc3\xc1\xe2\xc8\x40\xc1\xc4\xe5\xc1\xd5\xc3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf1\xf0\xf0\xf0\xf3\xc3\xd6\xd5\xe5\xc5\xd5\xc9\xc5\xd5\xc3\xc5\x40\xc3\xc8\xc5\xc3\xd2\x40\xc4\xc5\xc2\xc9\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf1\xf0\xf0\xf0\xf4\xc1\xe3\xd4\x40\xc3\xc1\xe2\xc8\x40\xc1\xc4\xe5\xc1\xd5\xc3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf1\xf0\xf0\xf0\xf5\xc9\xd5\xe3\xc5\xd9\xc5\xe2\xe3\x40\xc1\xd4\xd6\xe4\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf2\xf0\xf0\xf0\xf1\xc3\xc1\xe2\xc8\x40\xd7\xc1\xe8\xd4\xc5\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf2\xf0\xf0\xf0\xf2\xc5\xd3\xc5\xc3\xe3\xd9\xd6\xd5\xc9\xc3\x40\xd7\xc1\xe8\xd4\xc5\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf2\xf0\xf0\xf0\xf3\xc3\xc8\xc5\xc3\xd2\x40\xd7\xc1\xe8\xd4\xc5\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf3\xf0\xf0\xf0\xf1\xc3\xd9\xc5\xc4\xc9\xe3\x40\xe3\xd6\x40\xc1\xc3\xc3\xd6\xe4\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf3\xf0\xf0\xf0\xf2\xc3\xd9\xc5\xc4\xc9\xe3\x40\xe3\xd6\x40\xd7\xe4\xd9\xc3\xc8\xc1\xe2\xc5\x40\xc2\xc1\xd3\xc1\xd5\xc3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf3\xf0\xf0\xf0\xf3\xc3\xd9\xc5\xc4\xc9\xe3\x40\xe3\xd6\x40\xc3\xc1\xe2\xc8\x40\xc2\xc1\xd3\xc1\xd5\xc3\xc5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf4\xf0\xf0\xf0\xf1\xe9\xc5\xd9\xd6\x40\xc4\xd6\xd3\xd3\xc1\xd9\x40\xc1\xe4\xe3\xc8\xd6\xd9\xc9\xe9\xc1\xe3\xc9\xd6\xd5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf4\xf0\xf0\xf0\xf2\xd6\xd5\xd3\xc9\xd5\xc5\x40\xd7\xe4\xd9\xc3\xc8\xc1\xe2\xc5\x40\xc1\xe4\xe3\xc8\xd6\xd9\xc9\xe9\xc1\xe3\xc9\xd6\xd5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf4\xf0\xf0\xf0\xf3\xe3\xd9\xc1\xe5\xc5\xd3\x40\xc2\xd6\xd6\xd2\xc9\xd5\xc7\x40\xc1\xe4\xe3\xc8\xd6\xd9\xc9\xe9\xc1\xe3\xc9\xd6\xd5\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf5\xf0\xf0\xf0\xf1\xd9\xc5\xc6\xe4\xd5\xc4\x40\xc3\xd9\xc5\xc4\xc9\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf6\xf0\xf0\xf0\xf1\xc6\xd9\xc1\xe4\xc4\x40\xd9\xc5\xe5\xc5\xd9\xe2\xc1\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf6\xf0\xf0\xf0\xf2\xd5\xd6\xd5\x40\xc6\xd9\xc1\xe4\xc4\x40\xd9\xc5\xe5\xc5\xd9\xe2\xc1\xd3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
    b"\xf0\xf7\xf0\xf0\xf0\xf1\xe2\xc1\xd3\xc5\xe2\x40\xc4\xd9\xc1\xc6\xe3\x40\xc3\xd9\xc5\xc4\xc9\xe3\x40\xc1\xc4\xd1\xe4\xe2\xe3\xd4\xc5\xd5\xe3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0",
];

const EXPECTED_TYPE_KEYS: [&[u8]; 7] = [
    b"\xf0\xf1",
    b"\xf0\xf2",
    b"\xf0\xf3",
    b"\xf0\xf4",
    b"\xf0\xf5",
    b"\xf0\xf6",
    b"\xf0\xf7",
];

const EXPECTED_CATEGORY_KEYS: [&[u8]; 18] = [
    b"\xf0\xf1\xf0\xf0\xf0\xf1",
    b"\xf0\xf1\xf0\xf0\xf0\xf2",
    b"\xf0\xf1\xf0\xf0\xf0\xf3",
    b"\xf0\xf1\xf0\xf0\xf0\xf4",
    b"\xf0\xf1\xf0\xf0\xf0\xf5",
    b"\xf0\xf2\xf0\xf0\xf0\xf1",
    b"\xf0\xf2\xf0\xf0\xf0\xf2",
    b"\xf0\xf2\xf0\xf0\xf0\xf3",
    b"\xf0\xf3\xf0\xf0\xf0\xf1",
    b"\xf0\xf3\xf0\xf0\xf0\xf2",
    b"\xf0\xf3\xf0\xf0\xf0\xf3",
    b"\xf0\xf4\xf0\xf0\xf0\xf1",
    b"\xf0\xf4\xf0\xf0\xf0\xf2",
    b"\xf0\xf4\xf0\xf0\xf0\xf3",
    b"\xf0\xf5\xf0\xf0\xf0\xf1",
    b"\xf0\xf6\xf0\xf0\xf0\xf1",
    b"\xf0\xf6\xf0\xf0\xf0\xf2",
    b"\xf0\xf7\xf0\xf0\xf0\xf1",
];

fn rows_match(types: &[Vec<u8>], categories: &[Vec<u8>]) -> bool {
    types.iter().map(Vec::as_slice).eq(EXPECTED_TYPES)
        && categories.iter().map(Vec::as_slice).eq(EXPECTED_CATEGORIES)
}

fn row_difference(label: &str, actual: &[Vec<u8>], expected: &[&[u8]]) -> String {
    if actual.len() != expected.len() {
        return format!(
            "{label} record count: actual={}, expected={}",
            actual.len(),
            expected.len()
        );
    }
    for (index, (row, expected)) in actual.iter().zip(expected).enumerate() {
        if row.as_slice() != *expected {
            let offset = row
                .iter()
                .zip(*expected)
                .position(|(a, e)| a != e)
                .unwrap_or(row.len().min(expected.len()));
            return format!(
                "{label} row={index} length={}/{} first byte difference at={offset} actual={:?} expected={:?}",
                row.len(),
                expected.len(),
                row.get(offset),
                expected.get(offset)
            );
        }
    }
    format!("{label} rows match")
}

pub(super) fn require_extracted(
    types: &[Vec<u8>],
    categories: &[Vec<u8>],
) -> Result<(), CorpusProblem> {
    if !rows_match(types, categories) {
        return Err(CorpusProblem::new(
            "carddemo.db2.extract_bytes_drift",
            format!(
                "{}; {}",
                row_difference("PS types", types, &EXPECTED_TYPES),
                row_difference("PS categories", categories, &EXPECTED_CATEGORIES)
            ),
        ));
    }
    Ok(())
}

type VsamRows = (Vec<Vec<u8>>, Vec<Vec<u8>>);

fn vsam_rows(
    server: &ProductServer,
    name: &str,
    key_length: u32,
) -> Result<VsamRows, CorpusProblem> {
    let dataset = DatasetName::new(name, 128).map_err(|_| {
        CorpusProblem::new(
            "carddemo.db2.vsam_name_invalid",
            "VSAM dataset name is invalid",
        )
    })?;
    match server
        .dataset_service()
        .invoke(DatasetRequest::Attributes {
            dataset: dataset.clone(),
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::Attributes { attributes, .. }
            if attributes.organization == DatasetOrganization::KeySequenced
                && attributes.record_format == RecordFormat::Fixed
                && attributes.logical_record_length == 60
                && attributes.key_offset == Some(0)
                && attributes.key_length == Some(key_length)
                && attributes.ccsid == Some(37) => {}
        _ => {
            return Err(CorpusProblem::new(
                "carddemo.db2.vsam_layout_drift",
                "imported KSDS layout differs from pinned JCL",
            ));
        }
    }
    match server
        .dataset_service()
        .invoke(DatasetRequest::Read {
            dataset,
            member: None,
            key: None,
            max_records: 4096,
            control: Default::default(),
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::Records {
            records,
            identities,
            ..
        } => Ok((records, identities)),
        _ => Err(CorpusProblem::new(
            "carddemo.db2.vsam_read_drift",
            "imported KSDS read returned the wrong result",
        )),
    }
}

fn imported_rows(server: &ProductServer) -> Result<(VsamRows, VsamRows), CorpusProblem> {
    Ok((
        vsam_rows(server, "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS", 2)?,
        vsam_rows(server, "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS", 6)?,
    ))
}

fn imported_match(types: &VsamRows, categories: &VsamRows) -> bool {
    rows_match(&types.0, &categories.0)
        && types.1.iter().map(Vec::as_slice).eq(EXPECTED_TYPE_KEYS)
        && categories
            .1
            .iter()
            .map(Vec::as_slice)
            .eq(EXPECTED_CATEGORY_KEYS)
}

fn import_problem(types: &VsamRows, categories: &VsamRows) -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.db2.vsam_bytes_drift",
        format!(
            "{}; {}; {}; {}",
            row_difference("KSDS types", &types.0, &EXPECTED_TYPES),
            row_difference("KSDS type keys", &types.1, &EXPECTED_TYPE_KEYS),
            row_difference("KSDS categories", &categories.0, &EXPECTED_CATEGORIES),
            row_difference("KSDS category keys", &categories.1, &EXPECTED_CATEGORY_KEYS)
        ),
    )
}

fn compare_imported_rows(
    observations: &mut RouteObservations,
    types: &VsamRows,
    categories: &VsamRows,
) -> Result<(), CorpusProblem> {
    observations.compare(
        AuthorityKind::Journey,
        "CD.J14",
        "Db2-to-VSAM record bytes",
        !imported_match(types, categories),
        || Ok(import_problem(types, categories)),
    )
}

pub(super) fn compare_vsam(
    observations: &mut RouteObservations,
    server: &ProductServer,
) -> Result<(), CorpusProblem> {
    let (types, categories) = imported_rows(server)?;
    compare_imported_rows(observations, &types, &categories)
}

pub(super) fn require_vsam(server: &ProductServer) -> Result<(), CorpusProblem> {
    let (types, categories) = imported_rows(server)?;
    if !imported_match(&types, &categories) {
        return Err(import_problem(&types, &categories));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records() -> (VsamRows, VsamRows) {
        (
            (
                EXPECTED_TYPES.iter().map(|row| row.to_vec()).collect(),
                EXPECTED_TYPE_KEYS.iter().map(|row| row.to_vec()).collect(),
            ),
            (
                EXPECTED_CATEGORIES.iter().map(|row| row.to_vec()).collect(),
                EXPECTED_CATEGORY_KEYS
                    .iter()
                    .map(|row| row.to_vec())
                    .collect(),
            ),
        )
    }

    fn rejected(types: &VsamRows, categories: &VsamRows) {
        let mut observations = RouteObservations::default();
        let error = compare_imported_rows(&mut observations, types, categories).unwrap_err();
        assert_eq!(error.code, "carddemo.db2.vsam_bytes_drift");
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn exact_literal_records_add_only_the_owned_observation() {
        let (types, categories) = records();
        assert!(
            types
                .0
                .iter()
                .chain(&categories.0)
                .all(|row| row.len() == 60)
        );
        require_extracted(&types.0, &categories.0).unwrap();
        let mut observations = RouteObservations::default();
        compare_imported_rows(&mut observations, &types, &categories).unwrap();
        assert_eq!(
            observations.journeys(),
            &[("CD.J14".into(), vec!["Db2-to-VSAM record bytes".into()])]
        );
        assert!(observations.issues().is_empty());
    }

    #[test]
    fn changed_description_of_the_same_length_adds_no_observation() {
        let (mut types, categories) = records();
        types.0[1][2] ^= 1;
        rejected(&types, &categories);
        assert!(require_extracted(&types.0, &categories.0).is_err());
    }

    #[test]
    fn reordered_categories_add_no_observation() {
        let (types, mut categories) = records();
        categories.0.swap(0, 1);
        rejected(&types, &categories);
    }

    #[test]
    fn changed_padding_or_zero_suffix_adds_no_observation() {
        let (types, mut categories) = records();
        categories.0[0][55] = 0xf0;
        rejected(&types, &categories);
        let (mut types, categories) = records();
        types.0[0][59] = 0x40;
        rejected(&types, &categories);
    }

    #[test]
    fn missing_or_extra_record_adds_no_observation() {
        let (mut types, categories) = records();
        types.0.pop();
        rejected(&types, &categories);
        let (types, mut categories) = records();
        categories.0.push(categories.0[0].clone());
        rejected(&types, &categories);
    }

    #[test]
    fn incorrect_provider_keys_add_no_observation() {
        let (mut types, categories) = records();
        types.1.swap(0, 1);
        rejected(&types, &categories);
        let (types, mut categories) = records();
        categories.1[0].pop();
        rejected(&types, &categories);
    }

    #[test]
    #[ignore = "requires the exact clean CARDDEMO_CORPUS_DIR first-party input"]
    fn corpus_backed_db2_extraction_produces_the_owned_observation() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let inventory = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
        let (receipt, observations) =
            super::super::verify_carddemo_db2_observed(&inventory).unwrap();
        assert_eq!(receipt.status, "pass");
        assert_eq!(receipt.extraction_records, 25);
        assert_eq!(receipt.batch_routes, 5);
        assert_eq!(receipt.dataset_sha256.len(), 4);
        assert_eq!(
            observations.journeys(),
            &[(
                "CD.J14".into(),
                vec!["COBTUPDT".into(), "Db2-to-VSAM record bytes".into()]
            )]
        );
        assert!(observations.issues().is_empty());
        println!(
            "actual pinned-corpus PS extraction and two KSDS imports: 25 ordered 60-byte Cp037 records and provider keys compared, including reopen; exact compiled COBTUPDT command/success spool and complete table transition; two owned observations; no full-profile or licensed credit"
        );
    }
}
