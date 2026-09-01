use mainframe_env_compiler::{CobolCompiler, INTRINSIC_FUNCTIONS};
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
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.4/cobol/function-runtime-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.function-runtime.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<FunctionRuntimeFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionRuntimeFixture {
    id: String,
    row_id: String,
    name: String,
    expression: String,
    setup: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct FunctionRuntimeOutput {
    matched: bool,
    expected: String,
    actual: String,
}

struct FunctionRuntimeDriver;
struct FixtureAvailable;
struct ExecutedObservation;

static FUNCTION_RUNTIME_DRIVER: FunctionRuntimeDriver = FunctionRuntimeDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static EXECUTED_OBSERVATION: ExecutedObservation = ExecutedObservation;

pub fn verify_cobol_function_runtime_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-function-runtime-fixtures@1"
        || catalog.target_version != "0.4.0"
        || catalog.fixtures.len() != 82
    {
        return Err("COBOL function runtime fixture identity or denominator drifted".into());
    }
    let official = INTRINSIC_FUNCTIONS
        .iter()
        .map(|descriptor| (descriptor.id, (descriptor.row_id, descriptor.name)))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || official.get(fixture.id.as_str()).copied()
                != Some((fixture.row_id.as_str(), fixture.name.as_str()))
            || fixture.expression.len() > 1024
            || fixture.setup.len() > 2048
        {
            return Err(format!(
                "invalid COBOL function runtime fixture {}",
                fixture.id
            ));
        }
        let source = source(fixture);
        let analysis = CobolCompiler::default().analyze(&crate::source_bundle(&source));
        if !analysis.semantic.as_ref().is_some_and(|semantic| {
            semantic.intrinsic_calls.iter().any(|call| {
                INTRINSIC_FUNCTIONS
                    .iter()
                    .find(|descriptor| descriptor.kind == call.kind)
                    .is_some_and(|descriptor| descriptor.id == fixture.id)
            })
        }) {
            return Err(format!(
                "{} runtime fixture misses its typed intrinsic",
                fixture.id
            ));
        }
        crate::compile(&source).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.function-runtime.driver", limits)
            .map_err(|error| error.to_string())?,
        &FUNCTION_RUNTIME_DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.function-runtime.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.function-runtime.executed", limits)
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

impl ConformanceDriver for FunctionRuntimeDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let id = fixture_id(fixture)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL function runtime fixture {id}"))?;
        let output = execute_fixture(fixture).unwrap_or_else(|error| FunctionRuntimeOutput {
            matched: false,
            expected: "typed intrinsic is byte-identical at one-step and wide quanta".into(),
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
        let output: FunctionRuntimeOutput =
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

fn execute_fixture(fixture: &FunctionRuntimeFixture) -> Result<FunctionRuntimeOutput, String> {
    let artifact = crate::compile(&source(fixture))?;
    let mut digests = Vec::new();
    let mut previews = Vec::new();
    for ordinal in 0..2 {
        let mut invocation = crate::invocation(&artifact, 1024);
        for (name, value) in [
            ("cobol.current-date", b"2024022912345678+0000".as_slice()),
            ("cobol.when-compiled", b"2024022901020300+0000".as_slice()),
        ] {
            invocation.bindings.insert(
                name.into(),
                BoundedPayload::new(
                    "mainframe-env.cobol.datetime@1",
                    value.to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|error| error.to_string())?,
            );
        }
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .map_err(|error| format!("{error:?}"))?;
        let mut resume = MachineResume::Start;
        loop {
            match machine.drive(
                resume,
                Quantum::new(if ordinal == 0 { 1 } else { 512 }, 64 * 1024)
                    .ok_or("invalid function quantum")?,
            ) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::Completed(_) => break,
                other => {
                    return Err(format!(
                        "run={ordinal};terminal={other:?};position={}",
                        machine.position_summary()
                    ));
                }
            }
        }
        let result = machine
            .variable("RESULT")
            .ok_or("function RESULT storage is missing")?;
        if result.bytes().iter().all(|byte| *byte == 0xff) {
            return Err(format!(
                "run={ordinal};result retained its initialization bytes"
            ));
        }
        digests.push(format!("sha256:{:x}", Sha256::digest(result.bytes())));
        previews.push(format!(
            "{:?}",
            &result.bytes()[..result.bytes().len().min(48)]
        ));
    }
    Ok(FunctionRuntimeOutput {
        matched: digests[0] == digests[1],
        expected: "typed intrinsic is byte-identical at one-step and wide quanta".into(),
        actual: format!("digests={digests:?};previews={previews:?}"),
    })
}

fn source(fixture: &FunctionRuntimeFixture) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. FUNCTIONS. DATA DIVISION. WORKING-STORAGE SECTION. 01 ALPHA PIC A(8) VALUE 'ABCDEFGH'. 01 TEXT PIC X(16) VALUE '123.45'. 01 TEXT2 PIC X(8) VALUE '234.56'. 01 BITS PIC X(8) VALUE '01000001'. 01 HEX-TEXT PIC X(4) VALUE '4142'. 01 CURRENCY-TEXT PIC X(16) VALUE '$123.45'. 01 CURRENCY-SYMBOL PIC X VALUE '$'. 01 FLOAT-TEXT PIC X(16) VALUE '1.25E2'. 01 DATE-TEXT PIC X(10) VALUE '2024-02-29'. 01 TIME-TEXT PIC X(8) VALUE '12:34:56'. 01 INT PIC S9(9) BINARY VALUE 2. 01 INT2 PIC S9(9) BINARY VALUE 3. 01 NUM PIC S9(9)V99 COMP-3 VALUE 1.5. 01 NUM2 PIC S9(9)V99 COMP-3 VALUE 2.5. 01 NAT PIC N(16) NATIONAL. 01 NAT2 PIC N(16) NATIONAL. 01 UTF PIC U(16) BYTE-LENGTH 64 UTF-8. 01 PTR POINTER. 01 RESULT PIC X(512) VALUE HIGH-VALUES. PROCEDURE DIVISION. {} MOVE {} TO RESULT. STOP RUN.",
        fixture.setup, fixture.expression
    )
}

pub(super) fn assurance_sources() -> Result<Vec<(String, String, String)>, String> {
    Ok(fixture_catalog()?
        .fixtures
        .iter()
        .map(|fixture| {
            (
                format!("cobol.function-runtime.{}", fixture.id),
                fixture.row_id.clone(),
                source(fixture),
            )
        })
        .collect())
}

fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL function runtime fixture catalog: {error}"))
}

fn fixture_id(fixture: &FixtureRef) -> Result<&str, String> {
    fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign COBOL function runtime fixture {fixture}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_82_function_runtime_fixtures_are_deterministic_and_nontrivial() {
        verify_cobol_function_runtime_fixtures().unwrap();
        for fixture in fixture_catalog().unwrap().fixtures {
            let output = execute_fixture(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }
}
