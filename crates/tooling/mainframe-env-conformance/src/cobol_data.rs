use mainframe_env_compiler::DATA_DESCRIPTION_CLAUSES;
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.4/cobol/data-runtime-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.data-runtime.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<DataRuntimeFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DataRuntimeFixture {
    id: String,
    row_id: String,
    declarations: String,
    procedure: String,
    expected_output: String,
    expected_variables: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DataRuntimeOutput {
    matched: bool,
    expected: String,
    actual: String,
}

struct DataRuntimeDriver;
struct FixtureAvailable;
struct ExecutedObservation;

static DATA_RUNTIME_DRIVER: DataRuntimeDriver = DataRuntimeDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static EXECUTED_OBSERVATION: ExecutedObservation = ExecutedObservation;

pub fn verify_cobol_data_runtime_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-data-runtime-fixtures@1"
        || catalog.target_version != "0.4.0"
        || catalog.fixtures.len() != 17
    {
        return Err("COBOL data runtime fixture identity or denominator drifted".into());
    }
    let official = DATA_DESCRIPTION_CLAUSES
        .iter()
        .map(|descriptor| (descriptor.id, descriptor.row_id))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || official.get(fixture.id.as_str()).copied() != Some(fixture.row_id.as_str())
            || fixture.declarations.len() > 8192
            || fixture.procedure.len() > 8192
            || fixture.expected_output.len() > 8192
            || fixture.expected_variables.len() > 32
        {
            return Err(format!("invalid COBOL data runtime fixture {}", fixture.id));
        }
        crate::compile(&source(fixture)).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.data-runtime.driver", limits).map_err(|error| error.to_string())?,
        &DATA_RUNTIME_DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.data-runtime.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.data-runtime.executed", limits)
            .map_err(|error| error.to_string())?,
        &EXECUTED_OBSERVATION,
    )])
}

impl ConformancePredicate for FixtureAvailable {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let id = fixture_id(fixture)?;
        Ok(fixture_catalog()?
            .fixtures
            .iter()
            .any(|fixture| fixture.id == id))
    }
}

impl ConformanceDriver for DataRuntimeDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let id = fixture_id(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL data runtime fixture {id}"))?;
        let output = execute_fixture(fixture).unwrap_or_else(|error| DataRuntimeOutput {
            matched: false,
            expected: expected_summary(fixture),
            actual: error,
        });
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|error| error.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for ExecutedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output: DataRuntimeOutput =
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

fn execute_fixture(fixture: &DataRuntimeFixture) -> Result<DataRuntimeOutput, String> {
    let artifact = crate::compile(&source(fixture))?;
    let invocation = crate::invocation(&artifact, 8192);
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|error| format!("{error:?}"))?;
    let mut resume = MachineResume::Start;
    let completion = loop {
        match machine.drive(
            resume,
            Quantum::new(512, 64 * 1024).ok_or("invalid data fixture quantum")?,
        ) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::Completed(completion) => break completion,
            other => {
                return Err(format!(
                    "terminal={other:?};position={}",
                    machine.position_summary()
                ));
            }
        }
    };
    let variables = fixture
        .expected_variables
        .iter()
        .map(|(name, expected)| {
            let value = machine
                .variable(name)
                .ok_or_else(|| format!("missing variable {name}"))?;
            let actual = if expected.starts_with("hex:") {
                format!("hex:{}", hex(value.bytes()))
            } else {
                value.text()
            };
            Ok((name.clone(), actual))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let expected = expected_summary(fixture);
    let actual = format!(
        "output={:?};variables={variables:?}",
        String::from_utf8_lossy(completion.output.bytes())
    );
    Ok(DataRuntimeOutput {
        matched: completion.output.bytes() == fixture.expected_output.as_bytes()
            && variables == fixture.expected_variables,
        expected,
        actual,
    })
}

fn source(fixture: &DataRuntimeFixture) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. DATA-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. {} PROCEDURE DIVISION. {}",
        fixture.declarations, fixture.procedure
    )
}

pub(super) fn assurance_sources() -> Result<Vec<(String, String, String)>, String> {
    Ok(fixture_catalog()?
        .fixtures
        .iter()
        .map(|fixture| {
            (
                format!("cobol.data-runtime.{}", fixture.id),
                fixture.row_id.clone(),
                source(fixture),
            )
        })
        .collect())
}

fn expected_summary(fixture: &DataRuntimeFixture) -> String {
    format!(
        "output={:?};variables={:?}",
        fixture.expected_output, fixture.expected_variables
    )
}

fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL data runtime fixture catalog: {error}"))
}

fn fixture_id(fixture: &FixtureRef) -> Result<&str, String> {
    fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign COBOL data runtime fixture {fixture}"))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)] as char,
                DIGITS[usize::from(byte & 0x0f)] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_17_data_clause_runtime_fixtures_execute_exactly() {
        verify_cobol_data_runtime_fixtures().unwrap();
        for fixture in fixture_catalog().unwrap().fixtures {
            let output = execute_fixture(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }
}
