use mainframe_env_compiler::{
    COMPILER_DIRECTIVE_GROUPS, CobolCompiler, compiler_directing_descriptor,
};
use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformancePredicate, DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef,
    PredicateRef, RuntimeRegistry,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.3/cobol/frontend-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.frontend.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<FrontendFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontendFixture {
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
    Directing,
    DirectiveGroup,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    source: String,
    format: FixtureFormat,
    options: BTreeMap<String, String>,
    libraries: Vec<FixtureLibrary>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FixtureFormat {
    Fixed,
    Free,
    Variable,
}

impl From<FixtureFormat> for SourceFormat {
    fn from(value: FixtureFormat) -> Self {
        match value {
            FixtureFormat::Fixed => Self::Fixed,
            FixtureFormat::Free => Self::Free,
            FixtureFormat::Variable => Self::Variable,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureLibrary {
    path: String,
    source: String,
    format: FixtureFormat,
}

#[derive(Debug, Serialize)]
struct FrontendOutput {
    total: usize,
    accepted: usize,
    target_hits: usize,
    details: Vec<String>,
}

struct FrontendDriver;
struct FixtureAvailable;
struct AcceptedObservation;
struct RejectedObservation;

static FRONTEND_DRIVER: FrontendDriver = FrontendDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static ACCEPTED_OBSERVATION: AcceptedObservation = AcceptedObservation;
static REJECTED_OBSERVATION: RejectedObservation = RejectedObservation;

pub fn verify_cobol_frontend_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-frontend-fixtures@1"
        || catalog.target_version != "0.3.0"
        || catalog.fixtures.len() != 20
    {
        return Err("COBOL frontend fixture catalog identity or denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in catalog.fixtures {
        if !ids.insert(fixture.id.clone())
            || !rows.insert(fixture.row_id.clone())
            || fixture.valid.is_empty()
            || fixture.invalid.is_empty()
        {
            return Err(format!("invalid COBOL frontend fixture {}", fixture.id));
        }
        for case in fixture.valid.iter().chain(&fixture.invalid) {
            build_bundle(case)?;
        }
    }
    Ok(())
}

pub fn cobol_frontend_runtime(
    spec: &CompiledSpec,
    limits: ConformanceLimits,
) -> Result<RuntimeRegistry<'static>, String> {
    verify_cobol_frontend_fixtures()?;
    RuntimeRegistry::new(
        spec,
        vec![(
            DriverRef::new("cobol.frontend.driver", limits).map_err(|error| error.to_string())?,
            &FRONTEND_DRIVER,
        )],
        vec![(
            PredicateRef::new("cobol.fixture.available", limits)
                .map_err(|error| error.to_string())?,
            &FIXTURE_AVAILABLE,
        )],
        vec![
            (
                ObservationRef::new("cobol.frontend.accepted", limits)
                    .map_err(|error| error.to_string())?,
                &ACCEPTED_OBSERVATION as &dyn ConformanceObservation,
            ),
            (
                ObservationRef::new("cobol.frontend.rejected", limits)
                    .map_err(|error| error.to_string())?,
                &REJECTED_OBSERVATION as &dyn ConformanceObservation,
            ),
        ],
        limits,
    )
    .map_err(|error| error.to_string())
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

impl ConformanceDriver for FrontendDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let (id, valid) = parse_fixture_ref(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL frontend fixture {id}"))?;
        let cases = if valid {
            &fixture.valid
        } else {
            &fixture.invalid
        };
        let mut output = FrontendOutput {
            total: cases.len(),
            accepted: 0,
            target_hits: 0,
            details: Vec::new(),
        };
        for (index, case) in cases.iter().enumerate() {
            let bundle = build_bundle(case)?;
            let analysis = CobolCompiler::default().analyze(&bundle);
            let Some(syntax) = analysis.syntax else {
                let message = analysis
                    .diagnostics
                    .first()
                    .map_or_else(|| "rejected".into(), |diagnostic| format!("{diagnostic:?}"));
                output.details.push(format!("case-{index}:{message}"));
                continue;
            };
            output.accepted += 1;
            let hit = match fixture.target_kind {
                TargetKind::Directing => syntax
                    .compiler_directing_statements()
                    .iter()
                    .any(|node| compiler_directing_descriptor(node.kind).id == fixture.target_id),
                TargetKind::DirectiveGroup => syntax.compiler_directives().iter().any(|node| {
                    COMPILER_DIRECTIVE_GROUPS
                        .iter()
                        .find(|entry| entry.group == node.group)
                        .is_some_and(|entry| entry.id == fixture.target_id)
                }),
            };
            output.target_hits += usize::from(hit);
            output.details.push(format!(
                "case-{index}:accepted={};target={hit}",
                analysis.completeness == mainframe_env_diagnostics::Completeness::Complete
            ));
        }
        let bytes = serde_json::to_vec(&output).map_err(|error| error.to_string())?;
        DriverOutput::new(bytes, ConformanceLimits::default()).map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for AcceptedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output = parse_output(output)?;
        let matched = output.total > 0
            && output.accepted == output.total
            && output.target_hits == output.total;
        ObservationCheck::new(
            matched,
            "all valid forms accepted with the generated typed target",
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
            "all invalid operand or placement forms rejected",
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
        .map_err(|error| format!("COBOL frontend fixture catalog: {error}"))
}

fn parse_fixture_ref(fixture: &FixtureRef) -> Result<(&str, bool), String> {
    let value = fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign fixture reference {fixture}"))?;
    if let Some(id) = value.strip_suffix(".valid") {
        Ok((id, true))
    } else if let Some(id) = value.strip_suffix(".invalid") {
        Ok((id, false))
    } else {
        Err(format!(
            "invalid COBOL frontend fixture reference {fixture}"
        ))
    }
}

fn build_bundle(case: &FixtureCase) -> Result<SourceBundle, String> {
    let limits = SourceLimits::default();
    let primary_path = LogicalPath::new("fixture/main.cbl", limits.max_path_bytes)
        .map_err(|error| error.to_string())?;
    let mut files = vec![
        SourceFile::input(
            primary_path.as_str(),
            case.source.as_bytes().to_vec(),
            case.format.into(),
            SourceEncoding::Utf8,
            limits,
        )
        .map_err(|error| error.to_string())?,
    ];
    let mut members = Vec::new();
    for library in &case.libraries {
        let path = LogicalPath::new(&library.path, limits.max_path_bytes)
            .map_err(|error| error.to_string())?;
        files.push(
            SourceFile::input(
                path.as_str(),
                library.source.as_bytes().to_vec(),
                library.format.into(),
                SourceEncoding::Utf8,
                limits,
            )
            .map_err(|error| error.to_string())?,
        );
        members.push(path);
    }
    if members.is_empty() {
        SourceBundle::new(
            &primary_path,
            files,
            case.options.clone(),
            Vec::new(),
            limits,
        )
        .map_err(|error| error.to_string())
    } else {
        let library =
            SourceLibrary::new("LIBRARY", members, limits).map_err(|error| error.to_string())?;
        SourceBundle::with_libraries(
            &primary_path,
            files,
            vec![library],
            case.options.clone(),
            Vec::new(),
            limits,
        )
        .map_err(|error| error.to_string())
    }
}

fn parse_output(output: &DriverOutput) -> Result<FrontendOutput, String> {
    serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())
}

impl<'de> Deserialize<'de> for FrontendOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            total: usize,
            accepted: usize,
            target_hits: usize,
            details: Vec<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            total: raw.total,
            accepted: raw.accepted,
            target_hits: raw.target_hits,
            details: raw.details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_catalog_is_closed_and_bundles_are_valid() {
        verify_cobol_frontend_fixtures().unwrap();
    }

    #[test]
    fn every_frontend_fixture_accepts_and_rejects_its_reviewed_classes() {
        let limits = ConformanceLimits::default();
        for fixture in fixture_catalog().unwrap().fixtures {
            let valid =
                FixtureRef::new(format!("cobol.frontend.{}.valid", fixture.id), limits).unwrap();
            let valid_output = FRONTEND_DRIVER.execute(&valid).unwrap();
            assert!(
                ACCEPTED_OBSERVATION
                    .evaluate(&valid_output)
                    .unwrap()
                    .matched
            );

            let invalid =
                FixtureRef::new(format!("cobol.frontend.{}.invalid", fixture.id), limits).unwrap();
            let invalid_output = FRONTEND_DRIVER.execute(&invalid).unwrap();
            assert!(
                REJECTED_OBSERVATION
                    .evaluate(&invalid_output)
                    .unwrap()
                    .matched
            );
        }
    }
}
