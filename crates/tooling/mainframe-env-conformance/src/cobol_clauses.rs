use mainframe_env_compiler::{
    CobolClauseKind, CobolCompiler, data_description_clause_descriptor,
    file_description_clause_descriptor,
};
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
    include_bytes!("../../../../conformance/0.3/cobol/semantic-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.semantic.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<SemanticFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticFixture {
    id: String,
    row_id: String,
    target_kind: TargetKind,
    target_id: String,
    valid: Vec<FixtureCase>,
    invalid: Vec<FixtureCase>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TargetKind {
    FileClause,
    DataClause,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    declaration: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct SemanticOutput {
    total: usize,
    accepted: usize,
    target_hits: usize,
    details: Vec<String>,
}

struct SemanticDriver;
struct FixtureAvailable;
struct AcceptedObservation;
struct RejectedObservation;

static SEMANTIC_DRIVER: SemanticDriver = SemanticDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static ACCEPTED_OBSERVATION: AcceptedObservation = AcceptedObservation;
static REJECTED_OBSERVATION: RejectedObservation = RejectedObservation;

pub fn verify_cobol_semantic_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-semantic-fixtures@1"
        || catalog.target_version != "0.3.0"
        || catalog.fixtures.len() != 27
    {
        return Err("COBOL semantic fixture identity or denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in catalog.fixtures {
        if !ids.insert(fixture.id.clone())
            || !rows.insert(fixture.row_id.clone())
            || fixture.valid.is_empty()
            || fixture.invalid.is_empty()
        {
            return Err(format!("invalid COBOL semantic fixture {}", fixture.id));
        }
        for case in fixture.valid.iter().chain(&fixture.invalid) {
            if case.declaration.is_empty() || case.declaration.len() > 8192 {
                return Err(format!("invalid COBOL semantic case {}", fixture.id));
            }
        }
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.semantic.driver", limits).map_err(|error| error.to_string())?,
        &SEMANTIC_DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.semantic.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![
        (
            ObservationRef::new("cobol.semantic.accepted", limits)
                .map_err(|error| error.to_string())?,
            &ACCEPTED_OBSERVATION,
        ),
        (
            ObservationRef::new("cobol.semantic.rejected", limits)
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

impl ConformanceDriver for SemanticDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let (id, valid) = parse_fixture_ref(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL semantic fixture {id}"))?;
        let cases = if valid {
            &fixture.valid
        } else {
            &fixture.invalid
        };
        let mut output = SemanticOutput {
            total: cases.len(),
            accepted: 0,
            target_hits: 0,
            details: Vec::new(),
        };
        for (index, case) in cases.iter().enumerate() {
            let analysis = CobolCompiler::default().analyze(&bundle(fixture.target_kind, case)?);
            let Some(semantic) = analysis.semantic else {
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
            let hit = match fixture.target_kind {
                TargetKind::FileClause => semantic.file_descriptions.iter().any(|description| {
                    description.clauses.iter().any(|clause| match clause.kind {
                        CobolClauseKind::File(kind) => {
                            file_description_clause_descriptor(kind).id == fixture.target_id
                        }
                        CobolClauseKind::Data(_) => false,
                    })
                }),
                TargetKind::DataClause => semantic.data_descriptions.iter().any(|description| {
                    description.clauses.iter().any(|clause| match clause.kind {
                        CobolClauseKind::Data(kind) => {
                            data_description_clause_descriptor(kind).id == fixture.target_id
                        }
                        CobolClauseKind::File(_) => false,
                    })
                }),
            };
            output.target_hits += usize::from(hit);
            output
                .details
                .push(format!("case-{index}:accepted=true;target={hit}"));
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
                && output.target_hits == output.total,
            "all valid clause forms accepted with the generated typed target",
            format!(
                "total={};accepted={};target_hits={};details={:?}",
                output.total, output.accepted, output.target_hits, output.details
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
            "all invalid clause operand or placement forms rejected",
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
        .map_err(|error| format!("COBOL semantic fixture catalog: {error}"))
}

fn parse_fixture_ref(fixture: &FixtureRef) -> Result<(&str, bool), String> {
    let value = fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign semantic fixture {fixture}"))?;
    if let Some(id) = value.strip_suffix(".valid") {
        Ok((id, true))
    } else if let Some(id) = value.strip_suffix(".invalid") {
        Ok((id, false))
    } else {
        Err(format!("invalid semantic fixture reference {fixture}"))
    }
}

fn bundle(kind: TargetKind, case: &FixtureCase) -> Result<SourceBundle, String> {
    let source = match kind {
        TargetKind::FileClause => format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CLAUSE. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD. DATA DIVISION. FILE SECTION. {}. 01 TEST-RECORD PIC X(80). WORKING-STORAGE SECTION. 77 RECORD-LENGTH PIC 9(4). 77 FILE-ID PIC X(8). PROCEDURE DIVISION. STOP RUN.",
            case.declaration
        ),
        TargetKind::DataClause => format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. CLAUSE. DATA DIVISION. WORKING-STORAGE SECTION. {}. PROCEDURE DIVISION. STOP RUN.",
            case.declaration
        ),
    };
    let limits = SourceLimits::default();
    let path =
        LogicalPath::new("clause.cbl", limits.max_path_bytes).map_err(|error| error.to_string())?;
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

fn parse_output(output: &DriverOutput) -> Result<SemanticOutput, String> {
    serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_clause_fixture_accepts_and_rejects_reviewed_classes() {
        verify_cobol_semantic_fixtures().unwrap();
        let limits = ConformanceLimits::default();
        for fixture in fixture_catalog().unwrap().fixtures {
            let valid =
                FixtureRef::new(format!("cobol.semantic.{}.valid", fixture.id), limits).unwrap();
            let valid_output = SEMANTIC_DRIVER.execute(&valid).unwrap();
            let valid_check = ACCEPTED_OBSERVATION.evaluate(&valid_output).unwrap();
            assert!(
                valid_check.matched,
                "{}: {}",
                fixture.id, valid_check.actual
            );
            let invalid =
                FixtureRef::new(format!("cobol.semantic.{}.invalid", fixture.id), limits).unwrap();
            let invalid_output = SEMANTIC_DRIVER.execute(&invalid).unwrap();
            let invalid_check = REJECTED_OBSERVATION.evaluate(&invalid_output).unwrap();
            assert!(
                invalid_check.matched,
                "{}: {}",
                fixture.id, invalid_check.actual
            );
        }
    }
}
