use mainframe_env_compiler::{CobolCompiler, procedure_statement_descriptor};
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.3/cobol/statement-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.statement.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<StatementFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatementFixture {
    id: String,
    row_id: String,
    target_id: String,
    valid: Vec<String>,
    invalid: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StatementOutput {
    total: usize,
    accepted: usize,
    target_hits: usize,
    provenance_hits: usize,
    details: Vec<String>,
}

struct StatementDriver;
struct FixtureAvailable;
struct AcceptedObservation;
struct RejectedObservation;

static STATEMENT_DRIVER: StatementDriver = StatementDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static ACCEPTED_OBSERVATION: AcceptedObservation = AcceptedObservation;
static REJECTED_OBSERVATION: RejectedObservation = RejectedObservation;

pub fn verify_cobol_statement_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-statement-fixtures@1"
        || catalog.target_version != "0.3.0"
        || catalog.fixtures.len() != 44
    {
        return Err("COBOL statement fixture identity or denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in catalog.fixtures {
        if !ids.insert(fixture.id.clone())
            || !rows.insert(fixture.row_id.clone())
            || fixture.valid.is_empty()
            || fixture.invalid.is_empty()
            || fixture
                .valid
                .iter()
                .chain(&fixture.invalid)
                .any(|source| source.is_empty() || source.len() > 8192)
        {
            return Err(format!("invalid COBOL statement fixture {}", fixture.id));
        }
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.statement.driver", limits).map_err(|error| error.to_string())?,
        &STATEMENT_DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.statement.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![
        (
            ObservationRef::new("cobol.statement.accepted", limits)
                .map_err(|error| error.to_string())?,
            &ACCEPTED_OBSERVATION,
        ),
        (
            ObservationRef::new("cobol.statement.rejected", limits)
                .map_err(|error| error.to_string())?,
            &REJECTED_OBSERVATION,
        ),
    ])
}

impl ConformancePredicate for FixtureAvailable {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let (id, _) = parse_fixture_ref(fixture)?;
        Ok(fixture_catalog()?
            .fixtures
            .iter()
            .any(|fixture| fixture.id == id))
    }
}

impl ConformanceDriver for StatementDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let (id, valid) = parse_fixture_ref(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL statement fixture {id}"))?;
        let cases = if valid {
            &fixture.valid
        } else {
            &fixture.invalid
        };
        let mut output = StatementOutput {
            total: cases.len(),
            accepted: 0,
            target_hits: 0,
            provenance_hits: 0,
            details: Vec::new(),
        };
        for (index, statement_source) in cases.iter().enumerate() {
            let analysis = CobolCompiler::default().analyze(&bundle(statement_source)?);
            let Some(hir) = analysis.hir else {
                output.details.push(format!(
                    "case-{index}:{}",
                    analysis
                        .diagnostics
                        .first()
                        .map_or_else(|| "rejected".into(), |diagnostic| format!("{diagnostic:?}"))
                ));
                continue;
            };
            output.accepted += 1;
            let matching = hir.statements.iter().find(|statement| {
                statement.official.is_some_and(|kind| {
                    procedure_statement_descriptor(kind).id == fixture.target_id
                })
            });
            output.target_hits += usize::from(matching.is_some());
            output.provenance_hits +=
                usize::from(matching.is_some_and(|statement| !statement.source.is_empty()));
            output.details.push(format!(
                "case-{index}:accepted=true;target={};provenance={}",
                matching.is_some(),
                matching.is_some_and(|statement| !statement.source.is_empty())
            ));
        }
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|error| error.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for AcceptedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output = parse_output(output)?;
        ObservationCheck::new(
            output.total > 0
                && output.accepted == output.total
                && output.target_hits == output.total
                && output.provenance_hits == output.total,
            "all valid statement forms accepted with typed identity and provenance",
            format!(
                "total={};accepted={};target_hits={};provenance_hits={};details={:?}",
                output.total,
                output.accepted,
                output.target_hits,
                output.provenance_hits,
                output.details
            ),
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for RejectedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output = parse_output(output)?;
        ObservationCheck::new(
            output.total > 0 && output.accepted == 0,
            "all invalid statement operand or option forms rejected",
            format!(
                "total={};accepted={};details={:?}",
                output.total, output.accepted, output.details
            ),
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL statement fixture catalog: {error}"))
}

fn parse_fixture_ref(fixture: &FixtureRef) -> Result<(&str, bool), String> {
    let value = fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign statement fixture {fixture}"))?;
    if let Some(id) = value.strip_suffix(".valid") {
        Ok((id, true))
    } else if let Some(id) = value.strip_suffix(".invalid") {
        Ok((id, false))
    } else {
        Err(format!("invalid statement fixture reference {fixture}"))
    }
}

fn bundle(statement: &str) -> Result<SourceBundle, String> {
    let source = format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. STMT. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS INDEXED RECORD KEY IS A. SELECT OUT-FILE ASSIGN TO OUTDD. DATA DIVISION. FILE SECTION. FD TEST-FILE. 01 TEST-RECORD. 05 A PIC 9 VALUE 1. 05 FILLER PIC X(79). FD OUT-FILE. 01 OUT-RECORD PIC X(80). SD SORT-FILE. 01 SORT-RECORD PIC X(80). WORKING-STORAGE SECTION. 01 B PIC 9 VALUE 2. 01 C PIC X(256). 01 PTR POINTER. 01 TABLE-GROUP. 05 TABLE-ITEM OCCURS 2 TIMES PIC X. PROCEDURE DIVISION. {}. TARGET. EXIT. TARGET-EXIT. EXIT. STOP RUN.",
        statement
    );
    let limits = SourceLimits::default();
    let path = LogicalPath::new("statement.cbl", limits.max_path_bytes)
        .map_err(|error| error.to_string())?;
    let file = SourceFile::input(
        path.as_str(),
        source.into_bytes(),
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| error.to_string())?;
    SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits)
        .map_err(|error| error.to_string())
}

fn parse_output(output: &DriverOutput) -> Result<StatementOutput, String> {
    serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_statement_fixture_accepts_and_rejects_reviewed_classes() {
        verify_cobol_statement_fixtures().unwrap();
        let limits = ConformanceLimits::default();
        for fixture in fixture_catalog().unwrap().fixtures {
            let valid =
                FixtureRef::new(format!("cobol.statement.{}.valid", fixture.id), limits).unwrap();
            let valid_output = STATEMENT_DRIVER.execute(&valid).unwrap();
            let valid_check = ACCEPTED_OBSERVATION.evaluate(&valid_output).unwrap();
            assert!(
                valid_check.matched,
                "{}: {}",
                fixture.id, valid_check.actual
            );
            let invalid =
                FixtureRef::new(format!("cobol.statement.{}.invalid", fixture.id), limits).unwrap();
            let invalid_output = STATEMENT_DRIVER.execute(&invalid).unwrap();
            let invalid_check = REJECTED_OBSERVATION.evaluate(&invalid_output).unwrap();
            assert!(
                invalid_check.matched,
                "{}: {}",
                fixture.id, invalid_check.actual
            );
        }
    }
}
