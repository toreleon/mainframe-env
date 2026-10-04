//! Selected MQ fixture driver. No installed/native/JES or licensed acceptance.
mod product;
#[cfg(test)]
mod tests;

use mainframe_env_coverage::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const EXPECTED: &str =
    include_str!("../../../../conformance/spec/v1/fixtures/mq-selected-expectations.json");
const SETUP: &str =
    include_str!("../../../../conformance/spec/v1/fixtures/mq-selected-empty-md1.json");
// Actual typed request emission source, not expected output or compiled COBOL.
const INPUT: &[u8] = include_bytes!("mq_selected/product/machine.rs");
const SCHEMA: &str = "mainframe-env.mq-selected-ir-observation@1";
static DRIVER: Driver = Driver;
static MEMORY_OBSERVATION: Observation = Observation("mq.memory.local-v1");
static SQLITE_OBSERVATION: Observation = Observation("mq.sqlite.local-v1");

/// Bind the finite selected MQ product driver into the sole shared runner.
/// Missing full-call obligations remain mandatory and pending in its ledger.
pub fn bind_mq_selected<'a>(
    spec: &CompiledSpec,
    drivers: &mut Vec<(DriverRef, &'a dyn ConformanceDriver)>,
    observations: &mut Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
    limits: ConformanceLimits,
) -> Result<(), SpecProblem> {
    let id = DriverRef::new("mq.selected.product", limits)?;
    if spec.registries().drivers().contains(&id) {
        // Join the frozen typed call authority, not a second MQ call inventory.
        let mut positions = 0;
        for call in mainframe_env_host_api::mq_mqi::MqMqiCall::ALL {
            let source = call.source();
            if !spec
                .rows()
                .any(|r| r.row_id().as_str() == source.official_row)
            {
                return Err(SpecProblem::UnknownRow(source.official_row.into()));
            }
            positions += source.source_positions.len();
        }
        if positions != 27 {
            return Err(SpecProblem::UnknownRow("MQ source-position closure".into()));
        }
        let actual_input = digest(INPUT);
        for name in ["mq.memory.local-v1", "mq.sqlite.local-v1"] {
            let fixture = FixtureRef::new(name, limits)?;
            if spec.registries().fixtures().get(&fixture) != Some(&actual_input) {
                return Err(SpecProblem::MalformedDocument(
                    "MQ fixture digest does not bind actual Consumer input source".into(),
                ));
            }
        }
        drivers.push((id, &DRIVER));
        observations.push((
            ObservationRef::new("mq.selected.memory.exact", limits)?,
            &MEMORY_OBSERVATION,
        ));
        observations.push((
            ObservationRef::new("mq.selected.sqlite.exact", limits)?,
            &SQLITE_OBSERVATION,
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    call: String,
    #[serde(deserialize_with = "required_nullable")]
    status: Option<(i32, i32)>,
    kind: String,
    #[serde(deserialize_with = "required_nullable")]
    body_hex: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    data_length: Option<i32>,
    #[serde(deserialize_with = "required_nullable")]
    backout_count: Option<i32>,
    #[serde(deserialize_with = "required_nullable")]
    resolved_queue: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    descriptor_fields: Option<Vec<String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema: String,
    fixture: String,
    fixture_digest: String,
    expectation_digest: String,
    setup_digest: String,
    steps: Vec<Step>,
    saf: SafTranscript,
    denied_saf: SafTranscript,
    replay_unchanged: bool,
    forbidden_mutation: bool,
    denial_error: String,
    core_completed_effects: usize,
    provider_receipts: usize,
    audited_effects: usize,
    sqlite_reopen_equal: bool,
}
struct Driver;
impl ConformanceDriver for Driver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let report = product::run(fixture.as_str())?;
        DriverOutput::new(
            serde_json::to_vec(&report).map_err(|e| e.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
struct Observation(&'static str);
impl ConformanceObservation for Observation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let report: Report = serde_json::from_slice(output.bytes()).map_err(|e| e.to_string())?;
        let expected: Expectations = serde_json::from_str(EXPECTED).map_err(|e| e.to_string())?;
        let matched = report.fixture == self.0
            && report.schema == SCHEMA
            && report.fixture_digest == digest(INPUT)
            && report.expectation_digest == digest(EXPECTED.as_bytes())
            && report.setup_digest == digest(SETUP.as_bytes())
            && report.steps == expected.steps
            && report.saf == expected.saf
            && report.denied_saf == expected.denied_saf
            && report.replay_unchanged
            && !report.forbidden_mutation
            && report.denial_error == "Unauthorized"
            && report.core_completed_effects == 15
            && report.provider_receipts == 15
            && report.audited_effects == 15
            && report.sqlite_reopen_equal;
        ObservationCheck::new(matched, format!("{SCHEMA}: independent 15-effect transcript, SAF, replay, audit/core/receipt closure"),
            String::from_utf8(output.bytes().to_vec()).map_err(|e| e.to_string())?, ConformanceLimits::default())
            .map_err(|e| e.to_string())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectations {
    steps: Vec<Step>,
    saf: SafTranscript,
    denied_saf: SafTranscript,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SafTranscript {
    execution: String,
    run_unit: String,
    invocation_key: String,
    observations: Vec<SafObservation>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SafObservation {
    sequence: u64,
    original_key: String,
    call: String,
    phase: String,
    principal: String,
    class: String,
    resource: String,
    intent: String,
    decision: String,
}
fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}
