use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const RECEIPT_ENV: &str = "MAINFRAME_ENV_COBOL65_LICENSED_ORACLE_RECEIPT";
const MAX_RECEIPT_BYTES: u64 = 4 * 1024 * 1024;
const COMPILER_OPTIONS: &[&str] = &[
    "ARCH(8)",
    "ARITH(EXTEND)",
    "CODEPAGE(037)",
    "LP(32)",
    "NUMPROC(NOPFD)",
    "OPT(0)",
    "SSRANGE",
];
const RUNTIME_OPTIONS: &[&str] = &["ALL31(ON)", "HEAP(,,,FREE)", "RPTOPTS(ON)", "TRAP(ON)"];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: String,
    baseline_id: String,
    product_level: String,
    compiler_options: Vec<String>,
    runtime_options: Vec<String>,
    candidate_digest: String,
    spec_digest: String,
    fixture_digest: String,
    normalization_rules: Vec<String>,
    cohorts: Cohorts,
    rows: Vec<ReceiptRow>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptRow {
    row_id: String,
    fixture_id: String,
    normalized_observation_digest: String,
    cohorts: Cohorts,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cohorts {
    positive: String,
    negative: String,
    boundary: String,
    condition: String,
    interaction: String,
}

impl Cohorts {
    fn all_pass(&self) -> bool {
        [
            &self.positive,
            &self.negative,
            &self.boundary,
            &self.condition,
            &self.interaction,
        ]
        .into_iter()
        .all(|status| status == "pass")
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct LicensedOutput {
    matched: bool,
    expected: String,
    actual: String,
}

struct Driver;
struct Available;
struct Exact;
static DRIVER: Driver = Driver;
static AVAILABLE: Available = Available;
static EXACT: Exact = Exact;

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.licensed.driver", limits).map_err(|error| error.to_string())?,
        &DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.licensed.receipt.available", limits)
            .map_err(|error| error.to_string())?,
        &AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.licensed.row-equivalent", limits)
            .map_err(|error| error.to_string())?,
        &EXACT,
    )])
}

impl ConformancePredicate for Available {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let Some((receipt, _)) = receipt_from_env()? else {
            return Ok(false);
        };
        Ok(receipt
            .rows
            .iter()
            .any(|row| row.fixture_id == fixture.as_str()))
    }
}

impl ConformanceDriver for Driver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let (receipt, bytes) =
            receipt_from_env()?.ok_or("licensed COBOL receipt is unavailable")?;
        let row = receipt
            .rows
            .iter()
            .find(|row| row.fixture_id == fixture.as_str())
            .ok_or("licensed COBOL receipt omits fixture")?;
        let expected = "all five licensed cohorts pass for the exact row and fixture".to_string();
        let output = LicensedOutput {
            matched: row.cohorts.all_pass(),
            expected,
            actual: format!(
                "row={};fixture={};observation={}",
                row.row_id, row.fixture_id, row.normalized_observation_digest
            ),
        };
        DriverOutput::with_oracle_receipt(
            serde_json::to_vec(&output).map_err(|error| error.to_string())?,
            format!("sha256:{:x}", Sha256::digest(bytes)),
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for Exact {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output: LicensedOutput =
            serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())?;
        ObservationCheck::new(
            output.matched,
            output.expected,
            output.actual,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

pub fn licensed_fixture_digest() -> String {
    let mut digest = Sha256::new();
    for bytes in [
        include_bytes!("../../../../conformance/0.4/cobol/statement-runtime-fixtures.json")
            .as_slice(),
        include_bytes!("../../../../conformance/0.4/cobol/function-runtime-fixtures.json")
            .as_slice(),
        include_bytes!("../../../../conformance/0.4/cobol/data-runtime-fixtures.json").as_slice(),
        include_bytes!("../../../../conformance/0.4/cobol/file-runtime-fixtures.json").as_slice(),
    ] {
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    format!("sha256:{:x}", digest.finalize())
}

pub fn verify_cobol_licensed_receipt_from_env() -> Result<bool, String> {
    receipt_from_env().map(|receipt| receipt.is_some())
}

fn receipt_from_env() -> Result<Option<(Receipt, Vec<u8>)>, String> {
    let Ok(path) = std::env::var(RECEIPT_ENV) else {
        return Ok(None);
    };
    let metadata = fs::metadata(&path).map_err(|error| format!("licensed receipt: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_RECEIPT_BYTES {
        return Err("licensed receipt is not a bounded regular file".into());
    }
    let bytes = fs::read(&path).map_err(|error| format!("licensed receipt: {error}"))?;
    let receipt: Receipt =
        serde_json::from_slice(&bytes).map_err(|error| format!("licensed receipt: {error}"))?;
    verify_receipt(&receipt)?;
    Ok(Some((receipt, bytes)))
}

fn verify_receipt(receipt: &Receipt) -> Result<(), String> {
    let expected = crate::cobol_assurance::assurance_rows()?;
    let rows = receipt
        .rows
        .iter()
        .map(|row| (row.fixture_id.clone(), row))
        .collect::<BTreeMap<_, _>>();
    if receipt.schema_version != "mainframe-env.cobol-licensed-differential-receipt@1"
        || receipt.baseline_id != "ibm-enterprise-cobol-6.5-2026-05-31"
        || receipt.product_level.is_empty()
        || receipt.product_level.len() > 256
        || receipt.compiler_options
            != COMPILER_OPTIONS
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>()
        || receipt.runtime_options
            != RUNTIME_OPTIONS
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>()
        || !digest(&receipt.candidate_digest)
        || !digest(&receipt.spec_digest)
        || receipt.fixture_digest != licensed_fixture_digest()
        || receipt.normalization_rules.is_empty()
        || receipt.normalization_rules.len() > 64
        || receipt
            .normalization_rules
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != receipt.normalization_rules.len()
        || !receipt.cohorts.all_pass()
        || rows.len() != 153
        || rows.keys().cloned().collect::<BTreeSet<_>>()
            != expected.keys().cloned().collect::<BTreeSet<_>>()
        || rows.iter().any(|(fixture, row)| {
            expected.get(fixture) != Some(&row.row_id)
                || !row.cohorts.all_pass()
                || !digest(&row.normalized_observation_digest)
        })
    {
        return Err("licensed COBOL receipt identity or row closure drifted".into());
    }
    Ok(())
}

fn digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licensed_denominator_and_fixture_digest_are_stable_without_an_oracle() {
        assert_eq!(crate::cobol_assurance::assurance_rows().unwrap().len(), 153);
        assert!(digest(&licensed_fixture_digest()));
    }
}
