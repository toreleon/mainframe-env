//! Noncredit preparation of publication-derived candidates using IR v1.
//! This wrapper has no acceptance operation and exports no verdict or ledger.
use crate::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateDocument {
    schema_version: String,
    review_status: String,
    coverage_credit: u8,
    spec: Value,
    rules: Vec<CandidateRule>,
    missing_classes: Vec<MissingClasses>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Publication-derived proposal, not a maintainer-approved rule.
pub struct CandidateRule {
    /// Bounded unique proposal identifier, without acceptance authority.
    pub id: String,
    /// Review explanation for the independent proposed expectations.
    pub rationale: String,
    /// Exact source identities and locators requiring human review.
    pub sources: Vec<CandidateSource>,
    /// Existing IR row/obligation/gate references covered by this proposal.
    pub bindings: Vec<CandidateBinding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Metadata-only source anchor; cache presence grants no execution credit.
pub struct CandidateSource {
    /// Registered publication snapshot identifier.
    pub baseline: String,
    /// Repository-relative topic manifest retaining hashes, not publication bodies.
    pub manifest: String,
    /// Exact product/version topic locator within that manifest.
    pub topic_path: String,
    /// Bare lowercase SHA-256 of the externally retained topic bytes.
    pub sha256: String,
    /// Bounded section or parser-line locator for maintainer source review.
    pub anchor: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Reference to one existing shared-IR case, without an official verdict.
pub struct CandidateBinding {
    /// Immutable comparison-catalog row identity.
    pub row_id: String,
    /// Independently declared obligation within the referenced row.
    pub obligation_id: String,
    /// Canonical shared gate slug; licensed differential is excluded.
    pub gate: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
/// Explicit unproved applicability classes retained for each proposed row.
pub struct MissingClasses {
    /// Comparison-catalog row whose complete acceptance remains open.
    pub row_id: String,
    /// Bounded review dispositions, not reduced official obligations.
    pub classes: Vec<String>,
}

/// Validated draft using the shared IR compiler, with no acceptance operation.
pub struct CompiledCandidate {
    spec: CompiledSpec,
    rules: Vec<CandidateRule>,
    missing_classes: Vec<MissingClasses>,
}

/// Diagnostics only: no canonical event, oracle receipt, or gate/row pass count.
#[derive(Debug)]
pub struct CandidatePreparation {
    /// Number of diagnostic runner checks, not a row or gate pass count.
    pub checks: usize,
    /// Expected/observed differences and their bounded source/replay locators.
    pub mismatches: Vec<String>,
}

fn invalid(detail: impl Into<String>) -> SpecProblem {
    SpecProblem::RuntimeFailure(format!("candidate: {}", detail.into()))
}

impl CompiledCandidate {
    /// Compile a bounded zero-credit draft and reject approval/oracle authority.
    /// Topic hash registration is checked by the caller's source resolver.
    pub fn compile_json(
        catalog_digest: &str,
        catalog: Vec<OfficialCatalogRow>,
        bytes: &[u8],
        limits: ConformanceLimits,
    ) -> Result<Self, SpecProblem> {
        if bytes.len() > limits.max_spec_bytes {
            return Err(SpecProblem::LimitExceeded("candidate document"));
        }
        let raw: CandidateDocument =
            serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
        if raw.schema_version != "mainframe-env.conformance-candidate@1"
            || raw.review_status != "pending-maintainer"
            || raw.coverage_credit != 0
        {
            return Err(invalid("human acceptance is outside candidate preparation"));
        }
        let spec_bytes = serde_json::to_vec(&raw.spec).map_err(|e| invalid(e.to_string()))?;
        let spec = CompiledSpec::compile_json(catalog_digest, catalog, &spec_bytes, limits)?;
        if !spec.registries().reviewed_rules().is_empty()
            || spec
                .rows()
                .any(|row| !row.reviewed_rules().is_empty() || row.oracle().is_some())
            || spec.cases().any(|case| {
                !case.reviewed_rules().is_empty()
                    || case.oracle().is_some()
                    || case.key().gate == CoverageGate::Differential
            })
        {
            return Err(invalid(
                "accepted rules and licensed results cannot enter a candidate",
            ));
        }
        if raw.rules.is_empty() || raw.rules.len() > limits.max_registry_entries {
            return Err(invalid("missing or excessive candidate rules"));
        }
        let mut ids = BTreeSet::new();
        let mut mapped = BTreeSet::new();
        for rule in &raw.rules {
            let id = ReviewedRuleRef::new(&rule.id, limits)?;
            if !ids.insert(id)
                || rule.rationale.is_empty()
                || rule.rationale.len() > limits.max_locator_bytes
                || rule.sources.is_empty()
                || rule.sources.len() > limits.max_refs_per_case
                || rule.bindings.is_empty()
                || rule.bindings.len() > limits.max_bindings
            {
                return Err(invalid("unbounded, duplicate or empty rule"));
            }
            for source in &rule.sources {
                OperationRef::new(&source.baseline, limits)?;
                for locator in [&source.manifest, &source.topic_path, &source.anchor] {
                    if locator.is_empty() || locator.len() > limits.max_locator_bytes {
                        return Err(invalid("unbounded source anchor"));
                    }
                }
                if source.sha256.len() != 64
                    || !source
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                {
                    return Err(invalid("invalid source digest"));
                }
            }
            for binding in &rule.bindings {
                let gate = CoverageGate::ALL
                    .into_iter()
                    .find(|g| g.slug() == binding.gate)
                    .ok_or_else(|| invalid("unknown candidate gate"))?;
                let key = BindingKey {
                    row_id: OfficialRowId::new(&binding.row_id, limits)?,
                    obligation_id: ObligationId::new(&binding.obligation_id, limits)?,
                    gate,
                };
                if spec.case(&key).is_none() {
                    return Err(invalid("rule references an unknown binding"));
                }
                mapped.insert(key);
            }
        }
        let expected = spec
            .cases()
            .map(|case| case.key().clone())
            .collect::<BTreeSet<_>>();
        if expected.is_empty() || mapped != expected {
            return Err(invalid("candidate rule/binding closure is incomplete"));
        }
        let mut missing_rows = BTreeSet::new();
        for missing in &raw.missing_classes {
            let row = OfficialRowId::new(&missing.row_id, limits)?;
            if !missing_rows.insert(row)
                || missing.classes.is_empty()
                || missing.classes.len() > limits.max_refs_per_case
                || missing
                    .classes
                    .iter()
                    .any(|class| class.is_empty() || class.len() > limits.max_locator_bytes)
            {
                return Err(invalid(
                    "missing-class disposition is unbounded or duplicate",
                ));
            }
        }
        if missing_rows != spec.rows().map(|row| row.row_id().clone()).collect() {
            return Err(invalid(
                "every proposed row requires explicit missing classes",
            ));
        }
        Ok(Self {
            spec,
            rules: raw.rules,
            missing_classes: raw.missing_classes,
        })
    }

    /// Proposed rules retained for source and expectation review.
    pub fn rules(&self) -> &[CandidateRule] {
        &self.rules
    }
    /// Explicit missing classes for every proposed catalog row.
    pub fn missing_classes(&self) -> &[MissingClasses] {
        &self.missing_classes
    }
    /// Ordinary shared-IR draft cases, without accepted reviewed-rule bindings.
    pub fn cases(&self) -> impl Iterator<Item = &ConformanceCase> {
        self.spec.cases()
    }
    /// Shared registry declarations validated by the existing compiler.
    pub fn registries(&self) -> &RegistryDeclarations {
        self.spec.registries()
    }

    /// Delegate to the one shared runner, then discard its draft events/ledger.
    /// There is deliberately no API exposing the draft as an accepted spec.
    pub fn prepare<'a>(
        &'a self,
        drivers: Vec<(DriverRef, &'a dyn ConformanceDriver)>,
        predicates: Vec<(PredicateRef, &'a dyn ConformancePredicate)>,
        observations: Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
        selection: &RunnerSelection,
        context: &RunnerContext,
        limits: ConformanceLimits,
    ) -> Result<CandidatePreparation, SpecProblem> {
        let runtime = RuntimeRegistry::new(&self.spec, drivers, predicates, observations, limits)?;
        let report = ConformanceRunner::new(&self.spec, runtime, limits).run(selection, context)?;
        let events = report
            .batches
            .iter()
            .flat_map(|batch| &batch.events)
            .collect::<Vec<_>>();
        Ok(CandidatePreparation {
            checks: events.len(),
            mismatches: events.into_iter().filter(|event| event.verdict != Verdict::Pass)
                .map(|event| {
                    let anchors = self.rules.iter().filter(|rule| rule.bindings.iter().any(|binding|
                        binding.row_id == event.key.row_id.as_str()
                            && binding.obligation_id == event.key.obligation_id.as_str()
                            && binding.gate == event.key.gate.slug()))
                        .flat_map(|rule| &rule.sources).map(|source|
                            format!("{}/{}#{}", source.baseline, source.topic_path, source.anchor))
                        .collect::<Vec<_>>();
                    format!("{} row={} obligation={} gate={} source={} anchors={anchors:?} expected={} actual={} replay={} --prepare-candidates",
                        event.test_id, event.key.row_id, event.key.obligation_id,
                        event.key.gate.slug(), event.source_locator, event.expected, event.actual, event.replay)
                })
                .collect(),
        })
    }
}
