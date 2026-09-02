use mainframe_env_compiler_api::PublishedArtifact;
use mainframe_env_coverage::{
    ConformanceDriver, ConformanceLimits, ConformanceObservation, ConformancePredicate,
    DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef, PredicateRef,
};
use mainframe_env_diagnostics::FailureCategory;
use mainframe_env_execution_api::{
    BoundedPayload, Invocation, InvocationLimits, Machine, MachineDrive, MachineResume, Quantum,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, Deserialize)]
struct AssuranceOutput {
    conditioned: bool,
    cancelled: bool,
    recovered: bool,
    quantum_invariant: bool,
    expected: String,
    actual: String,
}
struct Driver;
struct Available;
struct Conditioned;
struct Cancelled;
struct Recovered;
struct QuantumInvariant;
static DRIVER: Driver = Driver;
static AVAILABLE: Available = Available;
static CONDITIONED: Conditioned = Conditioned;
static CANCELLED: Cancelled = Cancelled;
static RECOVERED: Recovered = Recovered;
static QUANTUM_INVARIANT: QuantumInvariant = QuantumInvariant;

pub fn verify_cobol_assurance_sources() -> Result<(), String> {
    let sources = sources()?;
    if sources.len() != 153
        || sources
            .values()
            .map(|(row, _)| row)
            .collect::<BTreeSet<_>>()
            .len()
            != 153
    {
        return Err("COBOL assurance source denominator drifted".into());
    }
    for (source_id, (_, source)) in sources {
        crate::compile(&source).map_err(|e| format!("{source_id}: {e}"))?;
    }
    Ok(())
}
pub(super) fn runtime_drivers(
    limits: ConformanceLimits,
) -> Result<Vec<(DriverRef, &'static dyn ConformanceDriver)>, String> {
    Ok(vec![(
        DriverRef::new("cobol.assurance.driver", limits).map_err(|e| e.to_string())?,
        &DRIVER,
    )])
}
pub(super) fn runtime_predicates(
    limits: ConformanceLimits,
) -> Result<Vec<(PredicateRef, &'static dyn ConformancePredicate)>, String> {
    Ok(vec![(
        PredicateRef::new("cobol.assurance.fixture.available", limits)
            .map_err(|e| e.to_string())?,
        &AVAILABLE,
    )])
}
pub(super) fn runtime_observations(
    limits: ConformanceLimits,
) -> Result<Vec<(ObservationRef, &'static dyn ConformanceObservation)>, String> {
    Ok(vec![
        (
            ObservationRef::new("cobol.assurance.resource-conditioned", limits)
                .map_err(|e| e.to_string())?,
            &CONDITIONED,
        ),
        (
            ObservationRef::new("cobol.assurance.cancellation-conditioned", limits)
                .map_err(|e| e.to_string())?,
            &CANCELLED,
        ),
        (
            ObservationRef::new("cobol.assurance.checkpoint-recovered", limits)
                .map_err(|e| e.to_string())?,
            &RECOVERED,
        ),
        (
            ObservationRef::new("cobol.assurance.quantum-invariant", limits)
                .map_err(|e| e.to_string())?,
            &QUANTUM_INVARIANT,
        ),
    ])
}
impl ConformancePredicate for Available {
    fn evaluate(&self, f: &FixtureRef) -> Result<bool, String> {
        Ok(sources()?.contains_key(f.as_str()))
    }
}
impl ConformanceDriver for Driver {
    fn execute(&self, f: &FixtureRef) -> Result<DriverOutput, String> {
        let sources = sources()?;
        let (_, source) = sources.get(f.as_str()).ok_or("unknown assurance source")?;
        let output = execute(f.as_str(), source).unwrap_or_else(|actual| AssuranceOutput {
            conditioned: false,
            cancelled: false,
            recovered: false,
            quantum_invariant: false,
            expected: "resource exhaustion, cancellation, checkpoint, and quantum identity".into(),
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
        let o: AssuranceOutput = serde_json::from_slice(o.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            o.conditioned,
            o.expected,
            format!("conditioned={};{}", o.conditioned, o.actual),
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
impl ConformanceObservation for Cancelled {
    fn evaluate(&self, o: &DriverOutput) -> Result<ObservationCheck, String> {
        let o: AssuranceOutput = serde_json::from_slice(o.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            o.cancelled,
            o.expected,
            format!("cancelled={};{}", o.cancelled, o.actual),
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
impl ConformanceObservation for Recovered {
    fn evaluate(&self, o: &DriverOutput) -> Result<ObservationCheck, String> {
        let o: AssuranceOutput = serde_json::from_slice(o.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            o.recovered,
            o.expected,
            format!("recovered={};{}", o.recovered, o.actual),
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
impl ConformanceObservation for QuantumInvariant {
    fn evaluate(&self, o: &DriverOutput) -> Result<ObservationCheck, String> {
        let o: AssuranceOutput = serde_json::from_slice(o.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            o.quantum_invariant,
            o.expected,
            format!("quantum-invariant={};{}", o.quantum_invariant, o.actual),
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}

fn execute(source_id: &str, source: &str) -> Result<AssuranceOutput, String> {
    let artifact = crate::compile(source)?;
    let mut limited = assurance_invocation(&artifact, source_id)?;
    limited.limits.max_steps = 1;
    let mut machine =
        ReferenceMachine::from_binary(artifact.payload(), limited, CodecLimits::default())
            .map_err(|e| format!("{e:?}"))?;
    let conditioned = loop {
        match machine.drive(
            MachineResume::Start,
            Quantum::new(512, 64 * 1024).ok_or("quantum")?,
        ) {
            MachineDrive::Continue => {}
            MachineDrive::Failed(problem) => {
                break problem.category == FailureCategory::ResourceExhausted;
            }
            other => return Err(format!("resource terminal={other:?}")),
        }
    };

    let invocation = assurance_invocation(&artifact, source_id)?;
    let mut cancelled = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .map_err(|e| format!("{e:?}"))?;
    match cancelled.drive(
        MachineResume::Start,
        Quantum::new(1, 64 * 1024).ok_or("quantum")?,
    ) {
        MachineDrive::Continue => {}
        other => return Err(format!("cancellation setup={other:?}")),
    }
    let before_cancel = cancelled.snapshot();
    let cancel_terminal = cancelled.drive(
        MachineResume::Cancelled,
        Quantum::new(512, 64 * 1024).ok_or("quantum")?,
    );
    let cancelled_without_mutation = matches!(
        &cancel_terminal,
        MachineDrive::Failed(problem) if problem.category == FailureCategory::Cancelled
    ) && before_cancel == cancelled.snapshot();

    let mut first = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .map_err(|e| format!("{e:?}"))?;
    match first.drive(
        MachineResume::Start,
        Quantum::new(1, 64 * 1024).ok_or("quantum")?,
    ) {
        MachineDrive::Continue => {}
        other => return Err(format!("checkpoint setup={other:?}")),
    }
    let checkpoint = first.checkpoint().ok_or("checkpoint unavailable")?;
    let mut restored = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .map_err(|e| format!("{e:?}"))?;
    restored
        .restore_checkpoint(&checkpoint)
        .map_err(|e| format!("{e:?}"))?;
    let first_terminal = drive(&mut first)?;
    let restored_terminal = drive(&mut restored)?;
    let recovered = first_terminal == restored_terminal && first.snapshot() == restored.snapshot();
    let mut narrow =
        ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
            .map_err(|e| format!("{e:?}"))?;
    let narrow_terminal = drive_with_quantum(&mut narrow, 1)?;
    let quantum_invariant =
        first_terminal == narrow_terminal && first.snapshot() == narrow.snapshot();
    Ok(AssuranceOutput {
        conditioned,
        cancelled: cancelled_without_mutation,
        recovered,
        quantum_invariant,
        expected: "resource exhaustion, cancellation, checkpoint, and quantum identity".into(),
        actual: format!(
            "fixture={source_id};schema={};terminal={first_terminal:?};cancel={cancel_terminal:?};narrow={narrow_terminal:?}",
            checkpoint.schema(),
        ),
    })
}
fn drive(
    machine: &mut ReferenceMachine,
) -> Result<MachineDrive<mainframe_env_host_api::EffectRequest>, String> {
    drive_with_quantum(machine, 512)
}
fn drive_with_quantum(
    machine: &mut ReferenceMachine,
    max_steps: u32,
) -> Result<MachineDrive<mainframe_env_host_api::EffectRequest>, String> {
    let mut resume = MachineResume::Start;
    loop {
        match machine.drive(resume, Quantum::new(max_steps, 64 * 1024).ok_or("quantum")?) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                resume = MachineResume::HostResult(crate::cobol_runtime::effect_result(&effect)?)
            }
            terminal => return Ok(terminal),
        }
    }
}
fn assurance_invocation(
    artifact: &PublishedArtifact,
    source_id: &str,
) -> Result<Invocation, String> {
    let mut invocation = crate::invocation(artifact, 8192);
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
            .map_err(|e| e.to_string())?,
        );
    }
    if source_id.starts_with("cobol.statement-runtime.")
        || source_id.starts_with("cobol.file-runtime.")
    {
        for (logical, dataset) in [
            ("TEST-FILE", "USER.TEST"),
            ("OUT-FILE", "USER.OUTPUT"),
            ("INPUT-FILE", "USER.INPUT"),
        ] {
            invocation.bindings.insert(
                format!("cobol.dd.{logical}"),
                BoundedPayload::new(
                    "mainframe-env.dataset-name@1",
                    dataset.as_bytes().to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(invocation)
}
fn sources() -> Result<BTreeMap<String, (String, String)>, String> {
    let mut output = BTreeMap::new();
    for (source_id, row_id, source) in crate::cobol_runtime::assurance_sources()?
        .into_iter()
        .chain(crate::cobol_intrinsics::assurance_sources()?)
        .chain(crate::cobol_data::assurance_sources()?)
        .chain(crate::cobol_files::assurance_sources()?)
    {
        if output.insert(source_id, (row_id, source)).is_some() {
            return Err("duplicate assurance source".into());
        }
    }
    Ok(output)
}

pub(crate) fn assurance_rows() -> Result<BTreeMap<String, String>, String> {
    Ok(sources()?
        .into_iter()
        .map(|(fixture, (row, _))| (fixture, row))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_153_sources_close_resource_cancellation_recovery_and_quantum_gates() {
        verify_cobol_assurance_sources().unwrap();
        for (source_id, (_, source)) in sources().unwrap() {
            let output = execute(&source_id, &source).unwrap();
            assert!(
                output.conditioned
                    && output.cancelled
                    && output.recovered
                    && output.quantum_invariant,
                "{source_id}: {}",
                output.actual
            )
        }
    }
}
