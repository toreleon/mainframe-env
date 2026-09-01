use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{EffectResult, HostProblem};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.4/cobol/recovery-fixtures.json");
const FIXTURE_PREFIX: &str = "cobol.recovery.";
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: String,
    target_version: String,
    fixtures: Vec<Fixture>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    id: String,
    row_id: String,
    source: String,
    marker: Marker,
    expected_output: String,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Marker {
    DynamicValue,
    SortSecondRecord,
    SearchBranch,
    PerformRepetition,
    XmlEvent,
    DeclarativeHandler,
}
#[derive(Debug, Deserialize, Serialize)]
struct Output {
    matched: bool,
    expected: String,
    actual: String,
}
struct Driver;
struct Available;
struct Recovered;
static DRIVER: Driver = Driver;
static AVAILABLE: Available = Available;
static RECOVERED: Recovered = Recovered;

pub fn verify_cobol_recovery_fixtures() -> Result<(), String> {
    let catalog = catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-recovery-fixtures@1"
        || catalog.target_version != "0.4.0"
        || !(5..=512).contains(&catalog.fixtures.len())
    {
        return Err("COBOL recovery fixture denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || fixture.source.len() > 16384
            || fixture.expected_output.len() > 8192
        {
            return Err(format!("invalid COBOL recovery fixture {}", fixture.id));
        }
        crate::compile(&fixture.source).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}
pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.recovery.driver", limits).map_err(|e| e.to_string())?,
        &DRIVER,
    )])
}
pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.recovery.fixture.available", limits).map_err(|e| e.to_string())?,
        &AVAILABLE,
    )])
}
pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.recovery.exact", limits).map_err(|e| e.to_string())?,
        &RECOVERED,
    )])
}
impl ConformancePredicate for Available {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let id = id(fixture)?;
        Ok(catalog()?.fixtures.iter().any(|f| f.id == id))
    }
}
impl ConformanceDriver for Driver {
    fn execute(&self, reference: &FixtureRef) -> Result<DriverOutput, String> {
        let id = id(reference)?;
        let catalog = catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|f| f.id == id)
            .ok_or("unknown recovery fixture")?;
        let output = execute(fixture).unwrap_or_else(|actual| Output {
            matched: false,
            expected: format!("restart-output={:?}", fixture.expected_output),
            actual,
        });
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|e| e.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
impl ConformanceObservation for Recovered {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output: Output = serde_json::from_slice(output.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            output.matched,
            output.expected,
            output.actual,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}

fn execute(fixture: &Fixture) -> Result<Output, String> {
    let artifact = crate::compile(&fixture.source)?;
    let invocation = crate::invocation(&artifact, 8192);
    let mut first = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .map_err(|e| format!("{e:?}"))?;
    let mut resume = MachineResume::Start;
    for _ in 0..4096 {
        let reached = match fixture.marker {
            Marker::DynamicValue => first
                .variable("DYN-X")
                .is_some_and(|v| v.bytes() == b"HELLO"),
            Marker::SortSecondRecord => first.position_summary().contains("move [\"'AA01'\""),
            Marker::SearchBranch => {
                first.position_summary().contains("role=Some(\"branch\")")
                    && first.position_summary().contains("scope=Some(\"search\")")
            }
            Marker::PerformRepetition => first.variable("I").is_some_and(|value| {
                value
                    .text()
                    .parse::<u8>()
                    .is_ok_and(|value| (1..5).contains(&value))
            }),
            Marker::XmlEvent => first
                .variable("XML-EVENT")
                .is_some_and(|value| value.bytes() == b"CONTENT-CHARACTERS"),
            Marker::DeclarativeHandler => first
                .variable("DECL-MARKER")
                .is_some_and(|value| value.bytes() == b"Y"),
        };
        if reached {
            break;
        }
        match first.drive(resume, Quantum::new(1, 8192).ok_or("quantum")?) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect)
                if matches!(fixture.marker, Marker::DeclarativeHandler) =>
            {
                resume = MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Err(HostProblem::Condition {
                        name: "NOTFND".into(),
                        response: 13,
                        response2: 0,
                    }),
                });
            }
            other => return Err(format!("pre-checkpoint terminal={other:?}")),
        }
    }
    let checkpoint = first.checkpoint().ok_or("checkpoint unavailable")?;
    let mut restored =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|e| format!("{e:?}"))?;
    restored
        .restore_checkpoint(&checkpoint)
        .map_err(|e| format!("{e:?}"))?;
    let left = drive(&mut first)?;
    let right = drive(&mut restored)?;
    let matched = left == right && left == fixture.expected_output.as_bytes();
    Ok(Output {
        matched,
        expected: format!("restart-output={:?}", fixture.expected_output),
        actual: format!(
            "schema={};left={:?};right={:?}",
            checkpoint.schema(),
            String::from_utf8_lossy(&left),
            String::from_utf8_lossy(&right)
        ),
    })
}
fn drive(machine: &mut ReferenceMachine) -> Result<Vec<u8>, String> {
    loop {
        match machine.drive(
            MachineResume::Start,
            Quantum::new(512, 64 * 1024).ok_or("quantum")?,
        ) {
            MachineDrive::Continue => {}
            MachineDrive::Completed(done) => return Ok(done.output.bytes().to_vec()),
            other => return Err(format!("terminal={other:?}")),
        }
    }
}
fn catalog() -> Result<Catalog, String> {
    serde_json::from_slice(FIXTURE_BYTES).map_err(|e| e.to_string())
}
fn id(reference: &FixtureRef) -> Result<&str, String> {
    reference
        .as_str()
        .strip_prefix(FIXTURE_PREFIX)
        .ok_or_else(|| "foreign recovery fixture".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_recovery_fixtures_restart_exactly() {
        verify_cobol_recovery_fixtures().unwrap();
        for fixture in catalog().unwrap().fixtures {
            let output = execute(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }
}
