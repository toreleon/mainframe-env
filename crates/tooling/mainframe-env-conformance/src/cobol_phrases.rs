use mainframe_env_compiler::PROCEDURE_STATEMENTS;
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_execution_api::{Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{DatasetLockMode, DatasetReadControl, DatasetRequest, HostRequest};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../../conformance/0.4/cobol/statement-phrase-runtime-fixtures.json");
const PREFIX: &str = "cobol.statement-phrase-runtime.";

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
    obligation_id: String,
    source: String,
    expected_output: String,
    expected_effects: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Output {
    matched: bool,
    expected: String,
    actual: String,
}

struct Driver;
struct Available;
struct Exact;
static DRIVER: Driver = Driver;
static AVAILABLE: Available = Available;
static EXACT: Exact = Exact;

pub fn verify_cobol_statement_phrase_runtime_fixtures() -> Result<(), String> {
    let catalog = catalog()?;
    let official = PROCEDURE_STATEMENTS
        .iter()
        .map(|descriptor| descriptor.row_id)
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    let mut bindings = BTreeSet::new();
    if catalog.schema_version != "mainframe-env.cobol-statement-phrase-runtime-fixtures@1"
        || catalog.target_version != "0.4.0"
        || catalog.fixtures.is_empty()
        || catalog.fixtures.len() > 256
    {
        return Err("COBOL statement phrase fixture identity drifted".into());
    }
    for fixture in &catalog.fixtures {
        if !ids.insert(fixture.id.as_str())
            || !bindings.insert((fixture.row_id.as_str(), fixture.obligation_id.as_str()))
            || !official.contains(fixture.row_id.as_str())
            || !fixture.obligation_id.starts_with("phrase-")
            || fixture.source.len() > 16_384
            || fixture.expected_output.len() > 16_384
            || fixture.expected_effects.as_ref().is_some_and(|effects| {
                effects.len() > 64 || effects.iter().any(|effect| effect.len() > 256)
            })
        {
            return Err(format!(
                "invalid COBOL statement phrase fixture {}",
                fixture.id
            ));
        }
        crate::compile(&fixture.source).map_err(|error| format!("{}: {error}", fixture.id))?;
    }
    Ok(())
}

pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.statement-phrase-runtime.driver", limits)
            .map_err(|error| error.to_string())?,
        &DRIVER,
    )])
}

pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.statement-phrase-runtime.fixture.available", limits)
            .map_err(|error| error.to_string())?,
        &AVAILABLE,
    )])
}

pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![(
        ObservationRef::new("cobol.statement-phrase-runtime.executed", limits)
            .map_err(|error| error.to_string())?,
        &EXACT,
    )])
}

impl ConformancePredicate for Available {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        let id = fixture
            .as_str()
            .strip_prefix(PREFIX)
            .ok_or("foreign COBOL statement phrase fixture")?;
        Ok(catalog()?.fixtures.iter().any(|fixture| fixture.id == id))
    }
}

impl ConformanceDriver for Driver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let id = fixture
            .as_str()
            .strip_prefix(PREFIX)
            .ok_or("foreign COBOL statement phrase fixture")?;
        let catalog = catalog()?;
        let fixture = catalog
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .ok_or("unknown COBOL statement phrase fixture")?;
        let output = execute(fixture).unwrap_or_else(|actual| Output {
            matched: false,
            expected: format!("output={:?}", fixture.expected_output),
            actual,
        });
        DriverOutput::new(
            serde_json::to_vec(&output).map_err(|error| error.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|error| error.to_string())
    }
}

impl ConformanceObservation for Exact {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let output: Output =
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

fn execute(fixture: &Fixture) -> Result<Output, String> {
    let artifact = crate::compile(&fixture.source)?;
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        crate::invocation(&artifact, 16_384),
        CodecLimits::default(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let mut resume = MachineResume::Start;
    let mut effects = Vec::new();
    let completion = loop {
        match machine.drive(
            resume,
            Quantum::new(512, 64 * 1024).ok_or("invalid phrase quantum")?,
        ) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                effects.push(phrase_effect(&effect.request));
                resume = MachineResume::HostResult(crate::cobol_runtime::effect_result(&effect)?)
            }
            MachineDrive::Completed(completion) => break completion,
            other => return Err(format!("terminal={other:?};{}", machine.position_summary())),
        }
    };
    Ok(Output {
        matched: completion.output.bytes() == fixture.expected_output.as_bytes()
            && fixture
                .expected_effects
                .as_ref()
                .is_none_or(|expected| expected == &effects),
        expected: format!(
            "output={:?};effects={:?}",
            fixture.expected_output, fixture.expected_effects
        ),
        actual: format!(
            "output={:?};effects={effects:?}",
            String::from_utf8_lossy(completion.output.bytes()),
        ),
    })
}

fn phrase_effect(request: &HostRequest) -> String {
    let control = |control: &DatasetReadControl| {
        format!(
            "lock={},wait={}",
            match control.lock {
                DatasetLockMode::Default => "default",
                DatasetLockMode::Lock => "lock",
                DatasetLockMode::KeptLock => "kept-lock",
                DatasetLockMode::NoLock => "no-lock",
                DatasetLockMode::IgnoreLock => "ignore-lock",
            },
            match control.wait {
                None => "default",
                Some(true) => "wait",
                Some(false) => "no-wait",
            }
        )
    };
    match request {
        HostRequest::Dataset(DatasetRequest::Read { control: value, .. }) => {
            format!("dataset.read:{}", control(value))
        }
        HostRequest::Dataset(DatasetRequest::ReadNext { control: value, .. }) => {
            format!("dataset.read-next:{}", control(value))
        }
        HostRequest::Dataset(_) => "dataset.other".into(),
        HostRequest::Terminal(_) => "terminal".into(),
        HostRequest::Clock(_) => "clock".into(),
        HostRequest::Program(_) => "program".into(),
        _ => "host.other".into(),
    }
}

fn catalog() -> Result<Catalog, String> {
    serde_json::from_slice(FIXTURE_BYTES).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_phrase_fixtures_execute_exactly() {
        verify_cobol_statement_phrase_runtime_fixtures().unwrap();
        for fixture in catalog().unwrap().fixtures {
            let output = execute(&fixture).unwrap();
            assert!(output.matched, "{}: {}", fixture.id, output.actual);
        }
    }
}
