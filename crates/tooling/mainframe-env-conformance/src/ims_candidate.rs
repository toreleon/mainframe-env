//! Independent IMS candidate fixtures over the shared IR preparation boundary.
use mainframe_env_coverage::*;
use mainframe_env_host_api::ImsMetadataCatalog;
use mainframe_env_ims::ImsGenericLoadImage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod route;
#[cfg(test)]
mod tests;
mod trials;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Backend {
    Memory,
    Sqlite,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SeedScope {
    Hierarchy,
    RootsOnly,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Trial {
    UniqueRepeated,
    SequentialBoundary,
    ParentSequence,
    ParentRequired,
    ParentMismatch,
    HoldUniqueReplace,
    HoldNextReplace,
    HoldParentReplace,
    ReplaceWithoutHold,
    DeleteWithoutHold,
    ReplaceKeyForbidden,
    DeleteSubtree,
    MalformedSsa,
    SsaLimit,
    Denied,
    ReplayedNext,
    ReplaceRollback,
    ReplaceRestartReplay,
    DeleteRestartReplay,
    InterveningGet,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SegmentOutcome {
    name: String,
    data: Vec<u8>,
    parent_key: Option<Vec<u8>>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CallOutcome {
    status: Option<String>,
    problem: Option<String>,
    segments: Vec<SegmentOutcome>,
    affected: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StateOutcome {
    current: Option<Vec<u8>>,
    parentage: Option<Vec<u8>>,
    held: bool,
    database: Vec<Vec<u8>>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    calls: Vec<CallOutcome>,
    state: StateOutcome,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    id: String,
    backend: Backend,
    seed_scope: SeedScope,
    trial: Trial,
    expected: Outcome,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureDocument {
    schema_version: String,
    metadata: ImsMetadataCatalog,
    seed: ImsGenericLoadImage,
    fixtures: Vec<Fixture>,
}

pub struct ImsCandidateRuntime {
    metadata: ImsMetadataCatalog,
    seed: ImsGenericLoadImage,
    fixtures: Vec<Fixture>,
}

pub fn ims_candidate_runtime(bytes: &[u8]) -> Result<ImsCandidateRuntime, String> {
    if bytes.len() > 512 * 1024 {
        return Err("IMS fixture bytes exceed bound".into());
    }
    let document: FixtureDocument = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if document.schema_version != "mainframe-env.ims-db-fixtures@1"
        || document.fixtures.is_empty()
        || document.fixtures.len() > 64
        || document.seed.records.len() > 16
        || document
            .seed
            .records
            .iter()
            .any(|record| record.data.len() > 64)
    {
        return Err("IMS fixture document is empty, oversized or incompatible".into());
    }
    let mut ids = BTreeSet::new();
    for fixture in &document.fixtures {
        FixtureRef::new(&fixture.id, ConformanceLimits::default()).map_err(|e| e.to_string())?;
        if !ids.insert(&fixture.id)
            || matches!(fixture.trial, Trial::SequentialBoundary)
                != (fixture.seed_scope == SeedScope::RootsOnly)
            || fixture.expected.calls.len() > 16
            || serde_json::to_vec(&fixture.expected)
                .map_err(|e| e.to_string())?
                .len()
                > 8192
        {
            return Err("IMS fixture identity or observations exceed bounds".into());
        }
    }
    Ok(ImsCandidateRuntime {
        fixtures: document.fixtures,
        metadata: document.metadata,
        seed: document.seed,
    })
}

impl ImsCandidateRuntime {
    pub fn fixture_ids(&self) -> impl Iterator<Item = &str> {
        self.fixtures.iter().map(|fixture| fixture.id.as_str())
    }

    pub fn prepare(
        &self,
        candidate: &CompiledCandidate,
        selection: &RunnerSelection,
        context: &RunnerContext,
    ) -> Result<CandidatePreparation, String> {
        self.prepare_using(candidate, self, selection, context)
    }

    fn prepare_using(
        &self,
        candidate: &CompiledCandidate,
        driver: &dyn ConformanceDriver,
        selection: &RunnerSelection,
        context: &RunnerContext,
    ) -> Result<CandidatePreparation, String> {
        let limits = ConformanceLimits::default();
        let observations = self
            .fixtures
            .iter()
            .map(|fixture| Observation(fixture.expected.clone()))
            .collect::<Vec<_>>();
        let handlers = self
            .fixtures
            .iter()
            .zip(&observations)
            .map(|(fixture, observation)| {
                Ok((
                    ObservationRef::new(format!("observe.{}", fixture.id), limits)
                        .map_err(|e| e.to_string())?,
                    observation as &dyn ConformanceObservation,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        candidate
            .prepare(
                vec![(
                    DriverRef::new("ims.db.public-host", limits).map_err(|e| e.to_string())?,
                    driver,
                )],
                vec![],
                handlers,
                selection,
                context,
                limits,
            )
            .map_err(|e| e.to_string())
    }

    /// Representative observation perturbations; diagnostic adequacy, zero credit.
    pub fn mutation_checks(&self) -> Result<usize, String> {
        let fixture = self
            .fixtures
            .iter()
            .find(|fixture| matches!(fixture.trial, Trial::HoldUniqueReplace))
            .ok_or("missing mutation fixture")?;
        let observation = Observation(fixture.expected.clone());
        let mut mutants = Vec::new();
        let mut omitted = fixture.expected.clone();
        omitted.state.database[0][2] ^= 1;
        mutants.push(omitted);
        let mut condition = fixture.expected.clone();
        condition.calls[0].status = Some("GE".into());
        mutants.push(condition);
        let mut bytes = fixture.expected.clone();
        bytes.calls[0].segments[0].data[0] ^= 1;
        mutants.push(bytes);
        let mut hold = fixture.expected.clone();
        hold.state.held = !hold.state.held;
        mutants.push(hold);
        let mut position = fixture.expected.clone();
        position.state.parentage = None;
        mutants.push(position);
        let negative = self
            .fixtures
            .iter()
            .find(|f| matches!(f.trial, Trial::Denied))
            .ok_or("missing authorization fixture")?;
        let mut bypass = negative.expected.clone();
        bypass.calls[0].problem = None;
        bypass.calls[0].status = Some("  ".into());
        let check = |o: &Observation, mutant: &Outcome| -> Result<(), String> {
            let output = DriverOutput::new(
                serde_json::to_vec(mutant).map_err(|e| e.to_string())?,
                ConformanceLimits::default(),
            )
            .map_err(|e| e.to_string())?;
            if o.evaluate(&output)?.matched {
                return Err("IMS observation mutant survived".into());
            }
            Ok(())
        };
        for mutant in &mutants {
            check(&observation, mutant)?;
        }
        check(&Observation(negative.expected.clone()), &bypass)?;
        let forbidden = self
            .fixtures
            .iter()
            .find(|f| matches!(f.trial, Trial::ReplaceWithoutHold))
            .ok_or("missing forbidden-mutation fixture")?;
        let mut mutated = forbidden.expected.clone();
        mutated.state.database[0][2] ^= 1;
        check(&Observation(forbidden.expected.clone()), &mutated)?;
        Ok(mutants.len() + 2)
    }
}

impl ConformanceDriver for ImsCandidateRuntime {
    fn execute(&self, reference: &FixtureRef) -> Result<DriverOutput, String> {
        let fixture = self
            .fixtures
            .iter()
            .find(|fixture| fixture.id == reference.as_str())
            .ok_or_else(|| format!("unknown IMS fixture {reference}"))?;
        let actual = trials::execute(fixture, &self.metadata, &self.seed)?;
        DriverOutput::new(
            serde_json::to_vec(&actual).map_err(|e| e.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}

struct Observation(Outcome);
impl ConformanceObservation for Observation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let actual: Outcome = serde_json::from_slice(output.bytes()).map_err(|e| e.to_string())?;
        ObservationCheck::new(
            actual == self.0,
            serde_json::to_string(&self.0).map_err(|e| e.to_string())?,
            serde_json::to_string(&actual).map_err(|e| e.to_string())?,
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}
