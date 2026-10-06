use mainframe_env_compiler::{CobolCompiler, procedure_statement_descriptor};
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_diagnostics::{FailureCategory, Phase};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLimits,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] = include_bytes!(
    "../../../../conformance/subsystems/cobol/structure/cobol/statement-fixtures.json"
);
const FIXTURE_PREFIX: &str = "cobol.statement.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_subsystem: String,
    fixtures: Vec<StatementFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatementFixture {
    id: String,
    row_id: String,
    target_id: String,
    valid: Vec<StatementCase>,
    invalid: Vec<StatementCase>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum StatementCase {
    Source(String),
    Detailed(DetailedStatementCase),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailedStatementCase {
    source: String,
    class: String,
    #[serde(default)]
    format: CaseFormat,
    #[serde(default)]
    expected_targets: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CaseFormat {
    Fixed,
    #[default]
    Free,
    Variable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StatementOutput {
    total: usize,
    accepted: usize,
    target_hits: usize,
    sequence_hits: usize,
    statement_provenance_hits: usize,
    control_provenance_hits: usize,
    target_diagnostic_hits: usize,
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
    if catalog.schema_version != "mainframe-env.cobol-statement-fixtures@2"
        || catalog.target_subsystem != "cobol.structure"
        || catalog.fixtures.len() != 44
    {
        return Err("COBOL statement fixture identity or denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in catalog.fixtures {
        let valid_classes = fixture
            .valid
            .iter()
            .map(StatementCase::class)
            .collect::<BTreeSet<_>>();
        let invalid_classes = fixture
            .invalid
            .iter()
            .map(StatementCase::class)
            .collect::<BTreeSet<_>>();
        if !ids.insert(fixture.id.clone())
            || !rows.insert(fixture.row_id.clone())
            || fixture.valid.len() < 2
            || fixture.invalid.len() < 4
            || fixture.valid.iter().chain(&fixture.invalid).any(|case| {
                case.source().is_empty()
                    || case.source().len() > 8192
                    || case.class().is_empty()
                    || case.class().len() > 64
            })
            || fixture
                .valid
                .iter()
                .any(|case| case.expected_targets(&fixture.target_id).is_empty())
            || !valid_classes.contains("base")
            || !valid_classes.iter().any(|class| {
                matches!(
                    *class,
                    "alternative" | "continuation" | "same-line-sequence"
                )
            })
            || !invalid_classes
                .iter()
                .any(|class| matches!(*class, "missing-required" | "arity-boundary"))
            || !invalid_classes.contains("alphabetic-near-miss")
            || !invalid_classes.iter().any(|class| {
                matches!(
                    *class,
                    "duplicate-or-exclusive" | "reordered-or-cardinality" | "malformed-nesting"
                )
            })
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
        let output = execute_cases(fixture, cases)?;
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|error| error.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

fn execute_cases(
    fixture: &StatementFixture,
    cases: &[StatementCase],
) -> Result<StatementOutput, String> {
    let mut output = StatementOutput {
        total: cases.len(),
        accepted: 0,
        target_hits: 0,
        sequence_hits: 0,
        statement_provenance_hits: 0,
        control_provenance_hits: 0,
        target_diagnostic_hits: 0,
        details: Vec::new(),
    };
    for (index, case) in cases.iter().enumerate() {
        let analysis = CobolCompiler::default().analyze(&bundle(case)?);
        let Some(hir) = analysis.hir else {
            let targeted = analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.code().as_str() == "MECOB0102"
                    && diagnostic.phase() == Phase::Parse
                    && diagnostic.category() == FailureCategory::MalformedInput
                    && diagnostic.public_message().contains(&fixture.target_id)
            });
            output.target_diagnostic_hits += usize::from(targeted);
            output.details.push(format!(
                "case-{index}:class={};target-diagnostic={targeted};{}",
                case.class(),
                analysis
                    .diagnostics
                    .first()
                    .map_or_else(|| "rejected".into(), |diagnostic| format!("{diagnostic:?}"))
            ));
            continue;
        };
        output.accepted += 1;
        let boundary = hir
            .statements
            .iter()
            .position(|statement| {
                statement.kind == mainframe_env_compiler::StatementKind::Label
                    && statement.arguments == ["SCAFFOLD-BOUNDARY"]
            })
            .ok_or("statement fixture scaffold boundary is absent from HIR")?;
        let actual_targets = hir.statements[..boundary]
            .iter()
            .filter_map(|statement| {
                statement
                    .official
                    .map(|kind| procedure_statement_descriptor(kind).id.to_string())
            })
            .collect::<Vec<_>>();
        let expected_targets = case.expected_targets(&fixture.target_id);
        let target_hit = actual_targets
            .iter()
            .any(|target| target == &fixture.target_id);
        let sequence_hit = actual_targets == expected_targets;
        let statement_provenance = hir.statements[..boundary]
            .iter()
            .filter(|statement| statement.official.is_some())
            .all(|statement| !statement.source.is_empty());
        let boundary_node = hir
            .control_nodes
            .iter()
            .position(|node| node.statement == Some(boundary))
            .ok_or("statement fixture scaffold boundary has no control node")?;
        let control_provenance = hir.control_nodes[..boundary_node]
            .iter()
            .all(|node| !node.source.is_empty());
        output.target_hits += usize::from(target_hit);
        output.sequence_hits += usize::from(sequence_hit);
        output.statement_provenance_hits += usize::from(statement_provenance);
        output.control_provenance_hits += usize::from(control_provenance);
        output.details.push(format!(
                "case-{index}:class={};accepted=true;target={target_hit};sequence={sequence_hit};statement-provenance={statement_provenance};control-provenance={control_provenance};actual={actual_targets:?};expected={expected_targets:?}",
                case.class(),
            ));
    }
    Ok(output)
}

impl ConformanceObservation for AcceptedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output = parse_output(output)?;
        ObservationCheck::new(
            output.total > 0
                && output.accepted == output.total
                && output.target_hits == output.total
                && output.sequence_hits == output.total
                && output.statement_provenance_hits == output.total
                && output.control_provenance_hits == output.total,
            "all valid statement forms accepted with exact typed sequence and bounded provenance",
            format!(
                "total={};accepted={};target_hits={};sequence_hits={};statement_provenance_hits={};control_provenance_hits={};details={:?}",
                output.total,
                output.accepted,
                output.target_hits,
                output.sequence_hits,
                output.statement_provenance_hits,
                output.control_provenance_hits,
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
            output.total > 0
                && output.accepted == 0
                && output.target_diagnostic_hits == output.total,
            "all invalid statement forms rejected by their target grammar diagnostic",
            format!(
                "total={};accepted={};target_diagnostic_hits={};details={:?}",
                output.total, output.accepted, output.target_diagnostic_hits, output.details
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

impl StatementCase {
    fn source(&self) -> &str {
        match self {
            Self::Source(source) => source,
            Self::Detailed(case) => &case.source,
        }
    }

    fn class(&self) -> &str {
        match self {
            Self::Source(_) => "legacy",
            Self::Detailed(case) => &case.class,
        }
    }

    fn format(&self) -> CaseFormat {
        match self {
            Self::Source(_) => CaseFormat::Free,
            Self::Detailed(case) => case.format,
        }
    }

    fn expected_targets(&self, target: &str) -> Vec<String> {
        match self {
            Self::Detailed(case) if !case.expected_targets.is_empty() => {
                case.expected_targets.clone()
            }
            _ => vec![target.to_string()],
        }
    }
}

fn bundle(case: &StatementCase) -> Result<SourceBundle, String> {
    let free_source = format!(
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. STMT.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT TEST-FILE ASSIGN TO TESTDD\n ORGANIZATION IS INDEXED RECORD KEY IS A.\nSELECT OUT-FILE ASSIGN TO OUTDD.\nDATA DIVISION.\nFILE SECTION.\nFD TEST-FILE.\n01 TEST-RECORD.\n 05 A PIC 9 VALUE 1.\n 05 FILLER PIC X(79).\nFD OUT-FILE.\n01 OUT-RECORD PIC X(80).\nSD SORT-FILE.\n01 SORT-RECORD PIC X(80).\nWORKING-STORAGE SECTION.\n01 B PIC 9 VALUE 2.\n01 C PIC X(256).\n01 PTR POINTER.\n01 TABLE-GROUP.\n 05 TABLE-ITEM OCCURS 2 TIMES PIC X.\nPROCEDURE DIVISION.\n{}.\nSCAFFOLD-BOUNDARY. EXIT.\nTARGET. EXIT.\nTARGET-EXIT. EXIT.\nSTOP RUN.\n",
        case.source()
    );
    let (source, format) = match case.format() {
        CaseFormat::Free => (free_source, SourceFormat::Free),
        CaseFormat::Fixed | CaseFormat::Variable => {
            let format = match case.format() {
                CaseFormat::Fixed => SourceFormat::Fixed,
                CaseFormat::Variable => SourceFormat::Variable,
                CaseFormat::Free => unreachable!(),
            };
            let source = free_source
                .lines()
                .enumerate()
                .map(|(index, line)| format!("{:06} {line}\n", index + 1))
                .collect();
            (source, format)
        }
    };
    let limits = SourceLimits::default();
    let path = LogicalPath::new("statement.cbl", limits.max_path_bytes)
        .map_err(|error| error.to_string())?;
    let file = SourceFile::input(
        path.as_str(),
        source.into_bytes(),
        format,
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

    fn detailed_case(source: &str, class: &str, expected_targets: &[&str]) -> StatementCase {
        StatementCase::Detailed(DetailedStatementCase {
            source: source.to_string(),
            class: class.to_string(),
            format: CaseFormat::Free,
            expected_targets: expected_targets
                .iter()
                .map(|target| (*target).to_string())
                .collect(),
        })
    }

    fn fixture(target: &str) -> StatementFixture {
        StatementFixture {
            id: target.to_string(),
            row_id: "mutation:statement:0001".to_string(),
            target_id: target.to_string(),
            valid: Vec::new(),
            invalid: Vec::new(),
        }
    }

    fn check(
        output: &StatementOutput,
        observation: &dyn ConformanceObservation,
    ) -> ObservationCheck {
        let driver_output = DriverOutput::new(
            serde_json::to_vec(output).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        observation.evaluate(&driver_output).unwrap()
    }

    #[test]
    fn generic_success_and_statement_collapse_mutants_are_killed() {
        let cases = [detailed_case(
            "MOVE A TO B DISPLAY B",
            "same-line-sequence",
            &["move", "display"],
        )];
        let output = execute_cases(&fixture("move"), &cases).unwrap();
        assert!(check(&output, &ACCEPTED_OBSERVATION).matched);

        let mut generic_success = output.clone();
        generic_success.target_hits = 0;
        generic_success.sequence_hits = 0;
        assert!(!check(&generic_success, &ACCEPTED_OBSERVATION).matched);

        let mut collapsed_second_statement = output;
        collapsed_second_statement.sequence_hits = 0;
        assert!(!check(&collapsed_second_statement, &ACCEPTED_OBSERVATION).matched);
    }

    #[test]
    fn permissive_suffix_and_option_bypass_mutants_are_killed() {
        for (target, source) in [
            ("start", "START TEST-FILE BOGUS"),
            ("read", "READ TEST-FILE INTO A INTO B"),
        ] {
            let cases = [detailed_case(source, "option-bypass", &[])];
            let output = execute_cases(&fixture(target), &cases).unwrap();
            assert!(check(&output, &REJECTED_OBSERVATION).matched);

            let mut permissive_mutant = output;
            permissive_mutant.accepted = 1;
            permissive_mutant.target_diagnostic_hits = 0;
            assert!(!check(&permissive_mutant, &REJECTED_OBSERVATION).matched);
        }
    }

    #[test]
    fn permissive_operand_and_omitted_valid_phrase_mutants_are_killed() {
        let invalid = [detailed_case(
            "CLOSE TEST-FILE WITH BOGUS",
            "alphabetic-near-miss",
            &[],
        )];
        let rejected = execute_cases(&fixture("close"), &invalid).unwrap();
        assert!(check(&rejected, &REJECTED_OBSERVATION).matched);
        let mut permissive_operand = rejected;
        permissive_operand.accepted = 1;
        permissive_operand.target_diagnostic_hits = 0;
        assert!(!check(&permissive_operand, &REJECTED_OBSERVATION).matched);

        let valid = [detailed_case(
            "READ TEST-FILE WITH NO LOCK",
            "optional-phrase",
            &["read"],
        )];
        let accepted = execute_cases(&fixture("read"), &valid).unwrap();
        assert!(check(&accepted, &ACCEPTED_OBSERVATION).matched);
        let mut omitted_valid_phrase = accepted;
        omitted_valid_phrase.accepted = 0;
        omitted_valid_phrase.target_hits = 0;
        omitted_valid_phrase.sequence_hits = 0;
        assert!(!check(&omitted_valid_phrase, &ACCEPTED_OBSERVATION).matched);
    }
}
