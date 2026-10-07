use mainframe_env_compiler::{
    CobolClauseKind, CobolCompiler, CobolLayout, CobolUsage, DataCategory,
    data_description_clause_descriptor, file_description_clause_descriptor,
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

const FIXTURE_BYTES: &[u8] = include_bytes!(
    "../../../../conformance/subsystems/cobol/structure/cobol/semantic-fixtures.json"
);
const FIXTURE_PREFIX: &str = "cobol.semantic.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_subsystem: String,
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
    #[serde(default)]
    expect: Vec<LayoutExpectation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutExpectation {
    name: String,
    category: Option<String>,
    usage: Option<String>,
    byte_length: Option<usize>,
    offset: Option<usize>,
    length: Option<usize>,
    element_length: Option<usize>,
    alignment: Option<usize>,
    signed: Option<bool>,
    sign_leading: Option<bool>,
    sign_separate: Option<bool>,
    justified_right: Option<bool>,
    blank_when_zero: Option<bool>,
    synchronized: Option<bool>,
    occurs_min: Option<usize>,
    occurs_max: Option<usize>,
    unbounded: Option<bool>,
    alias_of: Option<String>,
    depending_on: Option<String>,
    dynamic: Option<bool>,
    dynamic_limit: Option<usize>,
    external_name: Option<String>,
    global: Option<bool>,
    volatile: Option<bool>,
    typedef: Option<bool>,
    allocated: Option<bool>,
    object_class: Option<String>,
    initial_hex: Option<String>,
    source_spans_min: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SemanticOutput {
    total: usize,
    accepted: usize,
    target_hits: usize,
    expectation_hits: usize,
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
        || catalog.target_subsystem != "cobol.structure"
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
            || fixture.valid.iter().any(|case| case.expect.is_empty())
        {
            return Err(format!("invalid COBOL semantic fixture {}", fixture.id));
        }
        for case in fixture.valid.iter().chain(&fixture.invalid) {
            if case.declaration.is_empty() || case.declaration.len() > 8192 {
                return Err(format!("invalid COBOL semantic case {}", fixture.id));
            }
            if case
                .expect
                .iter()
                .any(|expectation| expectation.name.is_empty())
            {
                return Err(format!("invalid COBOL layout expectation {}", fixture.id));
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
            expectation_hits: 0,
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
            let expectations_match = case.expect.iter().all(|expectation| {
                semantic
                    .layout(&expectation.name)
                    .is_some_and(|layout| layout_matches(layout, expectation))
            });
            output.expectation_hits += usize::from(expectations_match);
            output.details.push(format!(
                "case-{index}:accepted=true;target={hit};layout={expectations_match}"
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
                && output.expectation_hits == output.total,
            "all valid clause forms accepted with the generated typed target and reviewed layout",
            format!(
                "total={};accepted={};target_hits={};expectation_hits={};details={:?}",
                output.total,
                output.accepted,
                output.target_hits,
                output.expectation_hits,
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

fn layout_matches(layout: &CobolLayout, expected: &LayoutExpectation) -> bool {
    expected
        .category
        .as_deref()
        .is_none_or(|value| value == category_slug(layout.category))
        && expected
            .usage
            .as_deref()
            .is_none_or(|value| value == usage_slug(layout.usage))
        && expected
            .byte_length
            .is_none_or(|value| layout.byte_length == Some(value))
        && expected.offset.is_none_or(|value| value == layout.offset)
        && expected.length.is_none_or(|value| value == layout.length)
        && expected
            .element_length
            .is_none_or(|value| value == layout.element_length)
        && expected
            .alignment
            .is_none_or(|value| value == layout.alignment)
        && expected.signed.is_none_or(|value| value == layout.signed)
        && expected
            .sign_leading
            .is_none_or(|value| value == layout.sign_leading)
        && expected
            .sign_separate
            .is_none_or(|value| value == layout.sign_separate)
        && expected
            .justified_right
            .is_none_or(|value| value == layout.justified_right)
        && expected
            .blank_when_zero
            .is_none_or(|value| value == layout.blank_when_zero)
        && expected
            .synchronized
            .is_none_or(|value| value == layout.synchronized)
        && expected
            .occurs_min
            .is_none_or(|value| value == layout.occurs_min)
        && expected
            .occurs_max
            .is_none_or(|value| value == layout.occurs)
        && expected
            .unbounded
            .is_none_or(|value| value == layout.unbounded)
        && expected
            .alias_of
            .as_deref()
            .is_none_or(|value| layout.alias_of.as_deref() == Some(value))
        && expected
            .depending_on
            .as_deref()
            .is_none_or(|value| layout.depending_on.as_deref() == Some(value))
        && expected.dynamic.is_none_or(|value| value == layout.dynamic)
        && expected
            .dynamic_limit
            .is_none_or(|value| layout.dynamic_limit == Some(value))
        && expected
            .external_name
            .as_deref()
            .is_none_or(|value| layout.external_name.as_deref() == Some(value))
        && expected.global.is_none_or(|value| value == layout.global)
        && expected
            .volatile
            .is_none_or(|value| value == layout.volatile)
        && expected.typedef.is_none_or(|value| value == layout.typedef)
        && expected
            .allocated
            .is_none_or(|value| value == layout.allocated)
        && expected
            .object_class
            .as_deref()
            .is_none_or(|value| layout.object_class.as_deref() == Some(value))
        && expected
            .initial_hex
            .as_deref()
            .is_none_or(|value| value == bytes_hex(&layout.initial))
        && expected
            .source_spans_min
            .is_none_or(|value| layout.source.len() >= value)
}

fn bytes_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

const fn category_slug(category: DataCategory) -> &'static str {
    match category {
        DataCategory::Alphabetic => "alphabetic",
        DataCategory::Alphanumeric => "alphanumeric",
        DataCategory::AlphanumericEdited => "alphanumeric-edited",
        DataCategory::Dbcs => "dbcs",
        DataCategory::National => "national",
        DataCategory::NationalEdited => "national-edited",
        DataCategory::Utf8 => "utf8",
        DataCategory::NumericDisplay => "numeric-display",
        DataCategory::NumericEdited => "numeric-edited",
        DataCategory::PackedDecimal => "packed-decimal",
        DataCategory::Binary => "binary",
        DataCategory::FloatShort => "float-short",
        DataCategory::FloatLong => "float-long",
        DataCategory::Index => "index",
        DataCategory::Pointer => "pointer",
        DataCategory::Pointer32 => "pointer-32",
        DataCategory::ProcedurePointer => "procedure-pointer",
        DataCategory::FunctionPointer => "function-pointer",
        DataCategory::ObjectReference => "object-reference",
        DataCategory::Group => "group",
        DataCategory::NationalGroup => "national-group",
        DataCategory::Utf8Group => "utf8-group",
        DataCategory::Condition => "condition",
        DataCategory::Rename => "rename",
    }
}

const fn usage_slug(usage: CobolUsage) -> &'static str {
    match usage {
        CobolUsage::Display => "display",
        CobolUsage::Display1 => "display-1",
        CobolUsage::National => "national",
        CobolUsage::Utf8 => "utf8",
        CobolUsage::Binary => "binary",
        CobolUsage::NativeBinary => "native-binary",
        CobolUsage::PackedDecimal => "packed-decimal",
        CobolUsage::FloatShort => "float-short",
        CobolUsage::FloatLong => "float-long",
        CobolUsage::Index => "index",
        CobolUsage::Pointer => "pointer",
        CobolUsage::Pointer32 => "pointer-32",
        CobolUsage::ProcedurePointer => "procedure-pointer",
        CobolUsage::FunctionPointer => "function-pointer",
        CobolUsage::ObjectReference => "object-reference",
        CobolUsage::Group => "group",
    }
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
