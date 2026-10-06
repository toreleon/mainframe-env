use mainframe_env_compiler::FILE_DESCRIPTION_CLAUSES;
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{
    BoundedPayload, InvocationLimits, Machine, MachineDrive, MachineResume, Quantum,
};
use mainframe_env_host_api::{
    DatasetRequest, DatasetResult, EffectResult, HostRequest, HostResult,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE_BYTES: &[u8] = include_bytes!(
    "../../../../conformance/subsystems/cobol/execution/cobol/file-runtime-fixtures.json"
);
const FIXTURE_PREFIX: &str = "cobol.file-runtime.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCatalog {
    schema_version: String,
    target_subsystem: String,
    fixtures: Vec<FileFixture>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileFixture {
    id: String,
    row_id: String,
    declaration: String,
    contract_fragment: String,
    expected_record_hex: String,
    expected_linage: i128,
}
#[derive(Debug, Deserialize, Serialize)]
struct FileOutput {
    matched: bool,
    expected: String,
    actual: String,
}
struct FileDriver;
struct FixtureAvailable;
struct ExecutedObservation;
static FILE_DRIVER: FileDriver = FileDriver;
static FIXTURE_AVAILABLE: FixtureAvailable = FixtureAvailable;
static EXECUTED_OBSERVATION: ExecutedObservation = ExecutedObservation;

pub fn verify_cobol_file_runtime_fixtures() -> Result<(), String> {
    let catalog = fixture_catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-file-runtime-fixtures@1"
        || catalog.target_subsystem != "cobol.execution"
        || catalog.fixtures.len() != 10
    {
        return Err("COBOL file runtime fixture identity or denominator drifted".into());
    }
    let official = FILE_DESCRIPTION_CLAUSES
        .iter()
        .map(|entry| (entry.id, entry.row_id))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || official.get(fixture.id.as_str()).copied() != Some(fixture.row_id.as_str())
            || fixture.declaration.len() > 4096
            || fixture.contract_fragment.len() > 1024
        {
            return Err(format!("invalid COBOL file runtime fixture {}", fixture.id));
        }
        crate::compile(&source(fixture)).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.file-runtime.driver", limits).map_err(|error| error.to_string())?,
        &FILE_DRIVER,
    )])
}
pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.file-runtime.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &FIXTURE_AVAILABLE,
    )])
}
pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.file-runtime.executed", limits)
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
impl ConformanceDriver for FileDriver {
    fn execute(&self, fixture_ref: &FixtureRef) -> Result<DriverOutput, String> {
        let id = fixture_id(fixture_ref)?;
        let catalog = fixture_catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or_else(|| format!("unknown COBOL file runtime fixture {id}"))?;
        let output = execute_fixture(fixture).unwrap_or_else(|actual| FileOutput {
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
        let output: FileOutput =
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

fn execute_fixture(fixture: &FileFixture) -> Result<FileOutput, String> {
    let artifact = crate::compile(&source(fixture))?;
    let mut invocation = crate::invocation(&artifact, 8192);
    invocation.bindings.insert(
        "cobol.dd.TEST-FILE".into(),
        BoundedPayload::new(
            "mainframe-env.dataset-name@1",
            b"USER.FILE".to_vec(),
            InvocationLimits::default(),
        )
        .map_err(|error| error.to_string())?,
    );
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|error| format!("{error:?}"))?;
    let contract = machine
        .file_contract("TEST-FILE")
        .ok_or("missing file contract")?
        .to_string();
    let mut resume = MachineResume::Start;
    let mut record = None;
    loop {
        match machine.drive(
            resume,
            Quantum::new(512, 64 * 1024).ok_or("invalid file fixture quantum")?,
        ) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                record = match &effect.request {
                    HostRequest::Dataset(
                        DatasetRequest::Append { records, .. }
                        | DatasetRequest::Write { records, .. },
                    ) => records.first().cloned(),
                    other => return Err(format!("unexpected file request {other:?}")),
                };
                resume = MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Dataset(DatasetResult::Mutated { version: 1 })),
                });
            }
            MachineDrive::Completed(_) => break,
            other => {
                return Err(format!(
                    "terminal={other:?};position={}",
                    machine.position_summary()
                ));
            }
        }
    }
    let record_hex = record
        .as_deref()
        .map(hex)
        .ok_or("file write emitted no record")?;
    let linage = machine
        .variable("LINAGE-COUNTER")
        .ok_or("missing LINAGE-COUNTER")?
        .text()
        .parse::<i128>()
        .map_err(|_| "invalid LINAGE-COUNTER")?;
    let expected = expected_summary(fixture);
    let actual = format!("contract={contract:?};record={record_hex};linage={linage}");
    Ok(FileOutput {
        matched: contract.contains(&fixture.contract_fragment)
            && record_hex == fixture.expected_record_hex
            && linage == fixture.expected_linage,
        expected,
        actual,
    })
}
fn source(fixture: &FileFixture) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. FILE-RUNTIME. ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL. SELECT TEST-FILE ASSIGN TO TESTDD ORGANIZATION IS SEQUENTIAL. DATA DIVISION. FILE SECTION. {}. 01 TEST-RECORD PIC X(8). WORKING-STORAGE SECTION. 01 VALUE-X PIC X(8) VALUE 'HELLO'. PROCEDURE DIVISION. WRITE TEST-RECORD FROM VALUE-X. STOP RUN.",
        fixture.declaration
    )
}

pub(super) fn assurance_sources() -> Result<Vec<(String, String, String)>, String> {
    Ok(fixture_catalog()?
        .fixtures
        .iter()
        .map(|fixture| {
            (
                format!("cobol.file-runtime.{}", fixture.id),
                fixture.row_id.clone(),
                source(fixture),
            )
        })
        .collect())
}
fn expected_summary(fixture: &FileFixture) -> String {
    format!(
        "contract contains {:?};record={};linage={}",
        fixture.contract_fragment, fixture.expected_record_hex, fixture.expected_linage
    )
}
fn fixture_catalog() -> Result<FixtureCatalog, String> {
    serde_json::from_slice(FIXTURE_BYTES)
        .map_err(|error| format!("COBOL file runtime fixture catalog: {error}"))
}
fn fixture_id(fixture: &FixtureRef) -> Result<&str, String> {
    fixture
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| format!("foreign COBOL file runtime fixture {fixture}"))
}
fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                D[usize::from(byte >> 4)] as char,
                D[usize::from(byte & 15)] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_10_file_clause_runtime_fixtures_execute_exactly() {
        verify_cobol_file_runtime_fixtures().unwrap();
        for fixture in fixture_catalog().unwrap().fixtures {
            let output = execute_fixture(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }
}
