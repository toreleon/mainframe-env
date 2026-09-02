use mainframe_env_compiler::SPECIAL_REGISTERS;
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{
    BoundedPayload, InvocationLimits, Machine, MachineDrive, MachineResume, Quantum,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.4/cobol/register-runtime-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.register-runtime.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<RegisterFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterFixture {
    id: String,
    row_id: String,
    name: String,
    declarations: String,
    procedure: String,
    expected_output: String,
    expected_variables: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RegisterOutput {
    matched: bool,
    expected: String,
    actual: String,
}

struct RegisterDriver;
struct FixtureAvailable;
struct ExecutedObservation;
static REGISTER_DRIVER: RegisterDriver = RegisterDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static EXECUTED_OBSERVATION: ExecutedObservation = ExecutedObservation;

pub fn verify_cobol_register_runtime_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-register-runtime-fixtures@1"
        || catalog.target_version != "0.4.0"
        || catalog.fixtures.len() != 28
    {
        return Err("COBOL register runtime fixture identity or denominator drifted".into());
    }
    let official = SPECIAL_REGISTERS
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            (
                entry.id,
                (
                    format!(
                        "ibm-enterprise-cobol-6.5-2026-05-31:special-registers:{:04}",
                        index + 1
                    ),
                    entry.name,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || official
                .get(fixture.id.as_str())
                .map(|(row, name)| (row.as_str(), *name))
                != Some((fixture.row_id.as_str(), fixture.name.as_str()))
            || fixture.declarations.len() > 4096
            || fixture.procedure.len() > 4096
            || fixture.expected_output.len() > 8192
            || fixture.expected_variables.len() > 16
        {
            return Err(format!(
                "invalid COBOL register runtime fixture {}",
                fixture.id
            ));
        }
        crate::compile(&source(fixture)).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.register-runtime.driver", limits)
            .map_err(|error| error.to_string())?,
        &REGISTER_DRIVER,
    )])
}
pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.register-runtime.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}
pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.register-runtime.executed", limits)
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
impl ConformanceDriver for RegisterDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let id = fixture_id(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL register runtime fixture {id}"))?;
        let output = execute_fixture(fixture).unwrap_or_else(|actual| RegisterOutput {
            matched: false,
            expected: expected_summary(fixture),
            actual,
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
        let output: RegisterOutput =
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

fn execute_fixture(fixture: &RegisterFixture) -> Result<RegisterOutput, String> {
    let artifact = crate::compile(&source(fixture))?;
    let mut invocation = crate::invocation(&artifact, 8192);
    invocation.bindings.insert(
        "cobol.when-compiled".into(),
        BoundedPayload::new(
            "mainframe-env.cobol.datetime@1",
            b"2024022901020300+0000".to_vec(),
            InvocationLimits::default(),
        )
        .map_err(|error| error.to_string())?,
    );
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|error| format!("{error:?}"))?;
    let mut resume = MachineResume::Start;
    let completion = loop {
        match machine.drive(
            resume,
            Quantum::new(512, 64 * 1024).ok_or("invalid register fixture quantum")?,
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
            let actual = if expected == "nonzero" {
                if value.bytes().iter().any(|byte| *byte != 0) {
                    "nonzero".into()
                } else {
                    "zero".into()
                }
            } else if expected.starts_with("hex:") {
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
        completion.output.bytes()
    );
    Ok(RegisterOutput {
        matched: completion.output.bytes() == fixture.expected_output.as_bytes()
            && variables == fixture.expected_variables,
        expected,
        actual,
    })
}

fn source(fixture: &RegisterFixture) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. REGISTER-RUNTIME. DATA DIVISION. WORKING-STORAGE SECTION. {} PROCEDURE DIVISION. {}",
        fixture.declarations, fixture.procedure
    )
}
fn expected_summary(fixture: &RegisterFixture) -> String {
    format!(
        "output={:?};variables={:?}",
        fixture.expected_output.as_bytes(),
        fixture.expected_variables
    )
}
fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL register runtime fixture catalog: {error}"))
}
fn fixture_id(fixture: &FixtureRef) -> Result<&str, String> {
    fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign COBOL register runtime fixture {fixture}"))
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
    fn all_28_register_runtime_fixtures_execute_exactly() {
        verify_cobol_register_runtime_fixtures().unwrap();
        for fixture in fixture_catalog().unwrap().fixtures {
            let output = execute_fixture(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }
}
