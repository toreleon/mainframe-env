use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{DatasetResult, EffectResult, HostProblem, HostResult};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const BYTES: &[u8] = include_bytes!("../../../../conformance/0.4/cobol/condition-fixtures.json");
const PREFIX: &str = "cobol.condition.";
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
    host_behavior: HostBehavior,
    expected_output: String,
    expected_terminal: Terminal,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum HostBehavior {
    None,
    ReadEof,
    CallFailure,
    StartNotFound,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Terminal {
    Completed,
    Condition,
    FailedMalformed,
}
#[derive(Debug, Deserialize, Serialize)]
struct Output {
    matched: bool,
    expected: String,
    actual: String,
}
struct Driver;
struct Available;
struct Conditioned;
static DRIVER: Driver = Driver;
static AVAILABLE: Available = Available;
static CONDITIONED: Conditioned = Conditioned;
pub fn verify_cobol_condition_fixtures() -> Result<(), String> {
    let catalog = catalog()?;
    if catalog.schema_version != "mainframe-env.cobol-condition-fixtures@1"
        || catalog.target_version != "0.4.0"
        || !(10..=512).contains(&catalog.fixtures.len())
    {
        return Err("COBOL condition fixture denominator drifted".into());
    }
    let mut ids = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !rows.insert(fixture.row_id.as_str())
            || fixture.source.len() > 16384
        {
            return Err(format!("invalid condition fixture {}", fixture.id));
        }
        crate::compile(&fixture.source).map_err(|e| format!("{}: {e}", fixture.id))?;
    }
    Ok(())
}
pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.condition.driver", limits).map_err(|e| e.to_string())?,
        &DRIVER,
    )])
}
pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.condition.fixture.available", limits)
            .map_err(|e| e.to_string())?,
        &AVAILABLE,
    )])
}
pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.condition.exact", limits).map_err(|e| e.to_string())?,
        &CONDITIONED,
    )])
}
impl ConformancePredicate for Available {
    fn evaluate(&self, f: &FixtureRef) -> Result<bool, String> {
        let id = id(f)?;
        Ok(catalog()?.fixtures.iter().any(|f| f.id == id))
    }
}
impl ConformanceDriver for Driver {
    fn execute(&self, r: &FixtureRef) -> Result<DriverOutput, String> {
        let id = id(r)?;
        let catalog = catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|f| f.id == id)
            .ok_or("unknown condition fixture")?;
        let output = execute(fixture).unwrap_or_else(|actual| Output {
            matched: false,
            expected: format!(
                "terminal={:?};output={:?}",
                fixture.expected_terminal, fixture.expected_output
            ),
            actual,
        });
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|e| e.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
impl ConformanceObservation for Conditioned {
    fn evaluate(&self, o: &DriverOutput) -> Result<ObservationCheck, String> {
        let o: Output = serde_json::from_slice(o.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            o.matched,
            o.expected,
            o.actual,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
fn execute(f: &Fixture) -> Result<Output, String> {
    let artifact = crate::compile(&f.source)?;
    let invocation = crate::invocation(&artifact, 8192);
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|e| format!("{e:?}"))?;
    let mut resume = MachineResume::Start;
    let (terminal, output) = loop {
        match machine.drive(resume, Quantum::new(512, 64 * 1024).ok_or("quantum")?) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                let outcome = match f.host_behavior {
                    HostBehavior::ReadEof => Ok(HostResult::Dataset(DatasetResult::Records {
                        records: Vec::new(),
                        identities: Vec::new(),
                        version: 1,
                    })),
                    HostBehavior::CallFailure => Err(HostProblem::NotFound),
                    HostBehavior::StartNotFound => Err(HostProblem::Condition {
                        name: "NOTFND".into(),
                        response: 13,
                        response2: 0,
                    }),
                    HostBehavior::None => {
                        return Err(format!("unexpected host call {:?}", effect.request));
                    }
                };
                resume = MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome,
                });
            }
            MachineDrive::Completed(done) => {
                break (Terminal::Completed, done.output.bytes().to_vec());
            }
            MachineDrive::Failed(_) => {
                break (Terminal::FailedMalformed, machine.output().to_vec());
            }
            MachineDrive::Condition(_) => {
                break (Terminal::Condition, machine.output().to_vec());
            }
            other => return Err(format!("terminal={other:?}")),
        }
    };
    let expected = format!(
        "terminal={:?};output={:?}",
        f.expected_terminal, f.expected_output
    );
    let actual = format!(
        "terminal={terminal:?};output={:?};host={:?}",
        String::from_utf8_lossy(&output),
        f.host_behavior
    );
    Ok(Output {
        matched: terminal == f.expected_terminal && output == f.expected_output.as_bytes(),
        expected,
        actual,
    })
}
fn catalog() -> Result<Catalog, String> {
    serde_json::from_slice(BYTES).map_err(|e| e.to_string())
}
fn id(r: &FixtureRef) -> Result<&str, String> {
    r.as_str()
        .strip_prefix(PREFIX)
        .ok_or_else(|| "foreign condition fixture".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Issue #187 root cause: a subscripted level-88 on an `OCCURS` item
    /// (COCRDLIC's `SELECT-BLANK`) must match a quoted-space `VALUE`
    /// literal, not compare it against a trimmed (thus emptied) field.
    #[test]
    fn level88_subscripted_condition_matches_a_space_literal_value() {
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. SL187. DATA DIVISION. WORKING-STORAGE SECTION. 01 WS-EDIT-SELECT-FLAGS PIC X(7) VALUE LOW-VALUES. 01 WS-EDIT-SELECT-ARRAY REDEFINES WS-EDIT-SELECT-FLAGS. 05 WS-EDIT-SELECT PIC X(1) OCCURS 7 TIMES. 88 SELECT-OK VALUES 'S', 'U'. 88 UPDATE-REQUESTED-ON VALUE 'U'. 88 SELECT-BLANK VALUES ' ', LOW-VALUES. 01 I PIC 9(1) VALUE 0. PROCEDURE DIVISION. MOVE 'U' TO WS-EDIT-SELECT(1). MOVE ' ' TO WS-EDIT-SELECT(2). PERFORM VARYING I FROM 1 BY 1 UNTIL I > 2 EVALUATE TRUE WHEN SELECT-OK(I) AND UPDATE-REQUESTED-ON(I) DISPLAY 'SELECTED' I WHEN SELECT-BLANK(I) DISPLAY 'BLANK' I WHEN OTHER DISPLAY 'INVALID' I END-EVALUATE END-PERFORM. STOP RUN.";
        let artifact = crate::compile(source).unwrap();
        match crate::execute(&artifact, 1024) {
            mainframe_env_execution_api::MachineDrive::Completed(done) => {
                assert_eq!(done.output.bytes(), b"SELECTED1\nBLANK2\n");
            }
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn all_condition_fixtures_are_exact() {
        verify_cobol_condition_fixtures().unwrap();
        for f in catalog().unwrap().fixtures {
            let o = execute(&f).unwrap();
            assert!(o.matched, "{}: {}", f.id, o.actual)
        }
    }
}
