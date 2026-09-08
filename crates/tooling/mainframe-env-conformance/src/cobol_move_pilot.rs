use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformanceScenarioDriver, DriverOutput, DriverRef, FixtureRef, ObservationCheck,
    ObservationRef, RuntimeRegistry, ScenarioId, ScenarioObservationBundle, ScenarioSpec,
    SpecProblem,
};
use mainframe_env_execution_api::MachineDrive;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("../../../../conformance/0.9/cobol/move-fixture.json");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CobolMovePilotReport {
    pub schema_version: String,
    pub fixture_digest: String,
    pub comparison_policy: String,
    pub output_hex: String,
    pub differential_credit: u8,
}

pub struct CobolMovePilotRuntime {
    case_driver: ScenarioOnlyDriver,
    readback_driver: ScenarioOnlyDriver,
    scenario_driver: MoveScenarioDriver,
    observation: MoveBytesObservation,
}

#[must_use]
pub const fn cobol_move_pilot_runtime() -> CobolMovePilotRuntime {
    CobolMovePilotRuntime {
        case_driver: ScenarioOnlyDriver,
        readback_driver: ScenarioOnlyDriver,
        scenario_driver: MoveScenarioDriver,
        observation: MoveBytesObservation,
    }
}

impl CobolMovePilotRuntime {
    pub fn bind<'a>(
        &'a self,
        spec: &CompiledSpec,
        drivers: &mut Vec<(DriverRef, &'a dyn ConformanceDriver)>,
        observations: &mut Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
        limits: ConformanceLimits,
    ) -> Result<Vec<(ScenarioId, &'a dyn ConformanceScenarioDriver)>, SpecProblem> {
        let scenario = ScenarioId::new("cobol.numeric-move.local", limits)?;
        if spec.scenario(&scenario).is_none() {
            return Ok(Vec::new());
        }
        drivers.push((
            DriverRef::new("cobol.numeric-move.product-path", limits)?,
            &self.case_driver,
        ));
        drivers.push((
            DriverRef::new("cobol.numeric-move.byte-readback", limits)?,
            &self.readback_driver,
        ));
        observations.push((
            ObservationRef::new("cobol.numeric-move.exact-bytes", limits)?,
            &self.observation,
        ));
        Ok(vec![(scenario, &self.scenario_driver)])
    }

    pub fn attach_scenarios<'a>(
        &'a self,
        spec: &CompiledSpec,
        runtime: RuntimeRegistry<'a>,
        limits: ConformanceLimits,
    ) -> Result<RuntimeRegistry<'a>, SpecProblem> {
        let scenario = ScenarioId::new("cobol.numeric-move.local", limits)?;
        if spec.scenario(&scenario).is_none() {
            return Ok(runtime);
        }
        runtime.with_scenario_drivers(spec, vec![(scenario, &self.scenario_driver)], limits)
    }
}

struct ScenarioOnlyDriver;

impl ConformanceDriver for ScenarioOnlyDriver {
    fn execute(&self, _fixture: &FixtureRef) -> Result<DriverOutput, String> {
        Err("COBOL numeric MOVE binding must execute through its ScenarioSpec".into())
    }
}

struct MoveScenarioDriver;

impl ConformanceScenarioDriver for MoveScenarioDriver {
    fn execute(&self, scenario: &ScenarioSpec) -> Result<ScenarioObservationBundle, String> {
        let report = run_cobol_move_pilot()?;
        let bytes = serde_json::to_vec(&report).map_err(|error| error.to_string())?;
        let observations = scenario
            .credits()
            .iter()
            .map(|key| {
                DriverOutput::new(bytes.clone(), ConformanceLimits::default())
                    .map(|output| (key.clone(), output))
                    .map_err(|problem| problem.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        ScenarioObservationBundle::new(observations, ConformanceLimits::default())
            .map_err(|problem| problem.to_string())
    }
}

struct MoveBytesObservation;

impl ConformanceObservation for MoveBytesObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let report: CobolMovePilotReport =
            serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())?;
        let fixture: Value = serde_json::from_str(FIXTURE).map_err(|error| error.to_string())?;
        let expected = fixture["expected_output_hex"]
            .as_str()
            .ok_or_else(|| "COBOL MOVE expected bytes are missing".to_string())?;
        let expected_policy = fixture["comparison_policy"]["version"]
            .as_str()
            .ok_or_else(|| "COBOL MOVE comparison policy is missing".to_string())?;
        let expected_identity = serde_json::json!({
            "schema_version": "mainframe-env.cobol-move-pilot-observation@1",
            "fixture_digest": digest(FIXTURE.as_bytes()),
            "comparison_policy": expected_policy,
            "differential_credit": 0,
        });
        let actual_identity = serde_json::json!({
            "schema_version": report.schema_version,
            "fixture_digest": report.fixture_digest,
            "comparison_policy": report.comparison_policy,
            "differential_credit": report.differential_credit,
        });
        ObservationCheck::new(
            report.output_hex == expected && expected_identity == actual_identity,
            format!("identity={expected_identity}; output={expected}"),
            format!("identity={actual_identity}; output={}", report.output_hex),
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

pub fn run_cobol_move_pilot() -> Result<CobolMovePilotReport, String> {
    let fixture: Value = serde_json::from_str(FIXTURE).map_err(|error| error.to_string())?;
    let source = fixture["program_source"]
        .as_str()
        .ok_or_else(|| "COBOL MOVE pilot source is missing".to_string())?;
    let artifact = crate::compile(source)?;
    let output = match crate::execute(&artifact, 4096) {
        MachineDrive::Completed(done) => done.output.bytes().to_vec(),
        other => return Err(format!("COBOL MOVE pilot did not complete: {other:?}")),
    };
    Ok(CobolMovePilotReport {
        schema_version: "mainframe-env.cobol-move-pilot-observation@1".into(),
        fixture_digest: digest(FIXTURE.as_bytes()),
        comparison_policy: fixture["comparison_policy"]["version"]
            .as_str()
            .ok_or_else(|| "COBOL MOVE comparison policy is missing".to_string())?
            .into(),
        output_hex: hex(&output),
        differential_credit: 0,
    })
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)] as char,
                DIGITS[usize::from(byte & 15)] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cobol_numeric_move_runs_source_to_exact_independent_bytes() {
        let report = run_cobol_move_pilot().unwrap();
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(
            report.output_hex,
            fixture["expected_output_hex"].as_str().unwrap()
        );
        assert_eq!(report.differential_credit, 0);
    }

    #[test]
    fn cobol_numeric_move_comparator_detects_sign_and_padding_changes() {
        let report = run_cobol_move_pilot().unwrap();
        let mut bytes = serde_json::to_vec(&report).unwrap();
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["output_hex"] = Value::String(report.output_hex.replacen("202d", "2020", 1));
        bytes = serde_json::to_vec(&value).unwrap();
        let output = DriverOutput::new(bytes, ConformanceLimits::default()).unwrap();
        assert!(!MoveBytesObservation.evaluate(&output).unwrap().matched);

        let mut stale = report;
        stale.fixture_digest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        let output = DriverOutput::new(
            serde_json::to_vec(&stale).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(!MoveBytesObservation.evaluate(&output).unwrap().matched);
    }
}
