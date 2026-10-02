//! Immutable six-gate official coverage contracts.

#![forbid(unsafe_code)]

mod candidate;
mod conformance;
mod gate;

pub use candidate::*;
pub use conformance::*;
pub use gate::CoverageGate;

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Stable identifier for one official-capability coverage row.
pub const COVERAGE_ROW_CONTRACT: &str = "mainframe-env.coverage-row@1";
/// Stable identifier for evidence attached to a coverage gate.
pub const COVERAGE_EVIDENCE_CONTRACT: &str = "mainframe-env.coverage-evidence@1";
/// Stable identifier for the immutable coverage ledger.
pub const COVERAGE_LEDGER_CONTRACT: &str = "mainframe-env.coverage-ledger@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceOutcome {
    Pass,
    Fail,
}

impl EvidenceOutcome {
    const fn slug(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleReceipt {
    pub environment_identity: String,
    pub product_identity: String,
    pub source_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceInput {
    pub row_id: String,
    pub gate: CoverageGate,
    pub sequence: u64,
    pub outcome: EvidenceOutcome,
    pub producer: String,
    pub artifact_digest: String,
    pub source_identity: String,
    pub oracle: Option<OracleReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    identity: String,
    input: EvidenceInput,
}

impl EvidenceRecord {
    pub fn new(input: EvidenceInput, limits: CoverageLimits) -> Result<Self, CoverageProblem> {
        validate_identity(&input.row_id, limits)?;
        validate_identity(&input.producer, limits)?;
        validate_identity(&input.source_identity, limits)?;
        validate_sha256(&input.artifact_digest)?;
        if input.sequence == 0 {
            return Err(CoverageProblem::InvalidSequence);
        }
        match &input.oracle {
            Some(oracle) => {
                validate_identity(&oracle.environment_identity, limits)?;
                validate_identity(&oracle.product_identity, limits)?;
                validate_sha256(&oracle.source_digest)?;
            }
            None if input.gate == CoverageGate::Differential
                && input.outcome == EvidenceOutcome::Pass =>
            {
                return Err(CoverageProblem::LicensedOracleRequired);
            }
            None => {}
        }
        let mut digest = Sha256::new();
        for field in [
            COVERAGE_EVIDENCE_CONTRACT,
            input.row_id.as_str(),
            input.gate.slug(),
            input.outcome.slug(),
            input.producer.as_str(),
            input.artifact_digest.as_str(),
            input.source_identity.as_str(),
        ] {
            digest_field(&mut digest, field.as_bytes());
        }
        digest_field(&mut digest, &input.sequence.to_be_bytes());
        if let Some(oracle) = &input.oracle {
            digest_field(&mut digest, oracle.environment_identity.as_bytes());
            digest_field(&mut digest, oracle.product_identity.as_bytes());
            digest_field(&mut digest, oracle.source_digest.as_bytes());
        }
        Ok(Self {
            identity: format!("sha256:{:x}", digest.finalize()),
            input,
        })
    }

    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    #[must_use]
    pub fn row_id(&self) -> &str {
        &self.input.row_id
    }

    #[must_use]
    pub const fn gate(&self) -> CoverageGate {
        self.input.gate
    }

    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.input.sequence
    }

    #[must_use]
    pub const fn outcome(&self) -> EvidenceOutcome {
        self.input.outcome
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GateState {
    NotApplicable,
    Pending,
    Failed,
    Passed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageRow {
    baseline_id: String,
    unit_id: String,
    row_id: String,
    applicable: BTreeSet<CoverageGate>,
    evidence: BTreeMap<CoverageGate, Vec<EvidenceRecord>>,
}

impl CoverageRow {
    pub fn new(
        baseline_id: impl Into<String>,
        unit_id: impl Into<String>,
        row_id: impl Into<String>,
        applicable: impl IntoIterator<Item = CoverageGate>,
        limits: CoverageLimits,
    ) -> Result<Self, CoverageProblem> {
        let baseline_id = baseline_id.into();
        let unit_id = unit_id.into();
        let row_id = row_id.into();
        validate_identity(&baseline_id, limits)?;
        validate_identity(&unit_id, limits)?;
        validate_identity(&row_id, limits)?;
        let applicable = applicable.into_iter().collect::<BTreeSet<_>>();
        if applicable.is_empty() || applicable.len() > CoverageGate::ALL.len() {
            return Err(CoverageProblem::InvalidApplicability);
        }
        Ok(Self {
            baseline_id,
            unit_id,
            row_id,
            applicable,
            evidence: BTreeMap::new(),
        })
    }

    pub fn record(&mut self, record: EvidenceRecord) -> Result<(), CoverageProblem> {
        if record.row_id() != self.row_id {
            return Err(CoverageProblem::WrongRow);
        }
        if !self.applicable.contains(&record.gate()) {
            return Err(CoverageProblem::GateNotApplicable);
        }
        let history = self.evidence.entry(record.gate()).or_default();
        if let Some(existing) = history
            .iter()
            .find(|item| item.sequence() == record.sequence())
        {
            return if existing == &record {
                Ok(())
            } else {
                Err(CoverageProblem::EvidenceConflict)
            };
        }
        if history
            .last()
            .is_some_and(|existing| existing.sequence() >= record.sequence())
        {
            return Err(CoverageProblem::StaleEvidence);
        }
        history.push(record);
        Ok(())
    }

    #[must_use]
    pub fn gate_state(&self, gate: CoverageGate) -> GateState {
        if !self.applicable.contains(&gate) {
            return GateState::NotApplicable;
        }
        match self
            .evidence
            .get(&gate)
            .and_then(|history| history.last())
            .map(EvidenceRecord::outcome)
        {
            None => GateState::Pending,
            Some(EvidenceOutcome::Fail) => GateState::Failed,
            Some(EvidenceOutcome::Pass) => GateState::Passed,
        }
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.applicable
            .iter()
            .all(|gate| self.gate_state(*gate) == GateState::Passed)
    }

    #[must_use]
    pub fn baseline_id(&self) -> &str {
        &self.baseline_id
    }

    #[must_use]
    pub fn unit_id(&self) -> &str {
        &self.unit_id
    }

    #[must_use]
    pub fn row_id(&self) -> &str {
        &self.row_id
    }

    pub fn evidence(&self) -> impl Iterator<Item = &EvidenceRecord> {
        self.evidence.values().flatten()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoverageCount {
    pub numerator: usize,
    pub denominator: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageSnapshot {
    baseline_id: String,
    catalog_digest: String,
    generation: u64,
    denominator: usize,
    rows: BTreeMap<String, CoverageRow>,
}

impl CoverageSnapshot {
    pub fn new(
        baseline_id: impl Into<String>,
        catalog_digest: impl Into<String>,
        generation: u64,
        denominator: usize,
        rows: Vec<CoverageRow>,
        limits: CoverageLimits,
    ) -> Result<Self, CoverageProblem> {
        let baseline_id = baseline_id.into();
        let catalog_digest = catalog_digest.into();
        validate_identity(&baseline_id, limits)?;
        validate_sha256(&catalog_digest)?;
        if generation == 0 || denominator == 0 || denominator > limits.max_rows {
            return Err(CoverageProblem::LimitExceeded);
        }
        let mut selected = BTreeMap::new();
        for row in rows {
            if row.baseline_id() != baseline_id {
                return Err(CoverageProblem::WrongBaseline);
            }
            let row_id = row.row_id().to_string();
            if selected.insert(row_id, row).is_some() {
                return Err(CoverageProblem::DuplicateRow);
            }
        }
        if selected.len() != denominator {
            return Err(CoverageProblem::DenominatorMismatch);
        }
        Ok(Self {
            baseline_id,
            catalog_digest,
            generation,
            denominator,
            rows: selected,
        })
    }

    #[must_use]
    pub fn count(&self, gate: CoverageGate) -> CoverageCount {
        self.rows.values().fold(
            CoverageCount {
                numerator: 0,
                denominator: 0,
            },
            |mut count, row| {
                match row.gate_state(gate) {
                    GateState::NotApplicable => {}
                    GateState::Passed => {
                        count.denominator += 1;
                        count.numerator += 1;
                    }
                    GateState::Pending | GateState::Failed => count.denominator += 1,
                }
                count
            },
        )
    }

    #[must_use]
    pub fn complete_rows(&self) -> usize {
        self.rows.values().filter(|row| row.is_complete()).count()
    }

    #[must_use]
    pub fn baseline_id(&self) -> &str {
        &self.baseline_id
    }

    #[must_use]
    pub fn catalog_digest(&self) -> &str {
        &self.catalog_digest
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn denominator(&self) -> usize {
        self.denominator
    }

    pub fn evidence(&self) -> impl Iterator<Item = &EvidenceRecord> {
        self.rows.values().flat_map(CoverageRow::evidence)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoverageLimits {
    pub max_identity_bytes: usize,
    pub max_rows: usize,
    pub max_evidence_records: usize,
    pub max_generations_per_baseline: usize,
}

impl Default for CoverageLimits {
    fn default() -> Self {
        Self {
            max_identity_bytes: 512,
            max_rows: 100_000,
            max_evidence_records: 1_000_000,
            max_generations_per_baseline: 1_024,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CoverageStore {
    anchors: BTreeMap<String, (String, usize)>,
    evidence: BTreeMap<String, EvidenceRecord>,
    evidence_keys: BTreeMap<(String, CoverageGate, u64), String>,
    snapshots: BTreeMap<String, BTreeMap<u64, CoverageSnapshot>>,
}

impl CoverageStore {
    pub fn append_evidence(
        &mut self,
        record: EvidenceRecord,
        limits: CoverageLimits,
    ) -> Result<(), CoverageProblem> {
        if let Some(existing) = self.evidence.get(record.identity()) {
            return if existing == &record {
                Ok(())
            } else {
                Err(CoverageProblem::EvidenceConflict)
            };
        }
        if self.evidence.len() >= limits.max_evidence_records {
            return Err(CoverageProblem::LimitExceeded);
        }
        let key = (
            record.row_id().to_string(),
            record.gate(),
            record.sequence(),
        );
        if self.evidence_keys.contains_key(&key) {
            return Err(CoverageProblem::EvidenceConflict);
        }
        self.evidence_keys
            .insert(key, record.identity().to_string());
        self.evidence.insert(record.identity().to_string(), record);
        Ok(())
    }

    pub fn commit(
        &mut self,
        snapshot: CoverageSnapshot,
        limits: CoverageLimits,
    ) -> Result<(), CoverageProblem> {
        for record in snapshot.evidence() {
            if self.evidence.get(record.identity()) != Some(record) {
                return Err(CoverageProblem::MissingEvidence);
            }
        }
        let baseline = snapshot.baseline_id().to_string();
        let anchor = (
            snapshot.catalog_digest().to_string(),
            snapshot.denominator(),
        );
        let anchor_is_new = if let Some(existing) = self.anchors.get(&baseline) {
            if existing != &anchor {
                return Err(CoverageProblem::DenominatorDrift);
            }
            false
        } else {
            true
        };
        let generations = self.snapshots.get(&baseline);
        if let Some(existing) = generations.and_then(|values| values.get(&snapshot.generation())) {
            return if existing == &snapshot {
                Ok(())
            } else {
                Err(CoverageProblem::GenerationConflict)
            };
        }
        if generations.is_some_and(|values| values.len() >= limits.max_generations_per_baseline) {
            return Err(CoverageProblem::LimitExceeded);
        }
        if generations
            .and_then(BTreeMap::last_key_value)
            .is_some_and(|(generation, _)| *generation >= snapshot.generation())
        {
            return Err(CoverageProblem::StaleGeneration);
        }
        if anchor_is_new {
            self.anchors.insert(baseline.clone(), anchor);
        }
        self.snapshots
            .entry(baseline)
            .or_default()
            .insert(snapshot.generation(), snapshot);
        Ok(())
    }

    #[must_use]
    pub fn latest(&self, baseline: &str) -> Option<&CoverageSnapshot> {
        self.snapshots
            .get(baseline)?
            .last_key_value()
            .map(|(_, value)| value)
    }

    #[must_use]
    pub fn evidence(&self, identity: &str) -> Option<&EvidenceRecord> {
        self.evidence.get(identity)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageProblem {
    InvalidIdentity,
    InvalidDigest,
    InvalidSequence,
    InvalidApplicability,
    LicensedOracleRequired,
    WrongRow,
    WrongBaseline,
    GateNotApplicable,
    EvidenceConflict,
    StaleEvidence,
    DuplicateRow,
    DenominatorMismatch,
    DenominatorDrift,
    MissingEvidence,
    GenerationConflict,
    StaleGeneration,
    LimitExceeded,
}

impl std::fmt::Display for CoverageProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "coverage contract rejected input: {self:?}")
    }
}

impl std::error::Error for CoverageProblem {}

fn validate_identity(value: &str, limits: CoverageLimits) -> Result<(), CoverageProblem> {
    if value.is_empty()
        || value.len() > limits.max_identity_bytes
        || value.chars().any(char::is_control)
    {
        Err(CoverageProblem::InvalidIdentity)
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str) -> Result<(), CoverageProblem> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(CoverageProblem::InvalidDigest)
    }
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(byte: u8) -> String {
        format!("sha256:{byte:064x}")
    }

    fn evidence(
        row: &str,
        gate: CoverageGate,
        sequence: u64,
        outcome: EvidenceOutcome,
    ) -> EvidenceRecord {
        EvidenceRecord::new(
            EvidenceInput {
                row_id: row.into(),
                gate,
                sequence,
                outcome,
                producer: "selected-route-test@1".into(),
                artifact_digest: sha(sequence as u8),
                source_identity: "candidate-source-identity".into(),
                oracle: (gate == CoverageGate::Differential).then(|| OracleReceipt {
                    environment_identity: "licensed-ibm-lpar-receipt".into(),
                    product_identity: "pinned-product-version".into(),
                    source_digest: sha(250),
                }),
            },
            CoverageLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn complete_is_derived_only_after_every_applicable_gate_passes() {
        let limits = CoverageLimits::default();
        let mut row =
            CoverageRow::new("baseline", "unit", "row", CoverageGate::ALL, limits).unwrap();
        for (index, gate) in CoverageGate::ALL.into_iter().enumerate() {
            let outcome = if gate == CoverageGate::Executed {
                EvidenceOutcome::Fail
            } else {
                EvidenceOutcome::Pass
            };
            row.record(evidence("row", gate, index as u64 + 1, outcome))
                .unwrap();
        }
        assert!(!row.is_complete());
        row.record(evidence(
            "row",
            CoverageGate::Executed,
            7,
            EvidenceOutcome::Pass,
        ))
        .unwrap();
        assert!(row.is_complete());
        assert_eq!(row.evidence().count(), 7);
    }

    #[test]
    fn differential_pass_requires_a_licensed_oracle_receipt() {
        let result = EvidenceRecord::new(
            EvidenceInput {
                row_id: "row".into(),
                gate: CoverageGate::Differential,
                sequence: 1,
                outcome: EvidenceOutcome::Pass,
                producer: "test".into(),
                artifact_digest: sha(1),
                source_identity: "source".into(),
                oracle: None,
            },
            CoverageLimits::default(),
        );
        assert_eq!(result.unwrap_err(), CoverageProblem::LicensedOracleRequired);
    }

    #[test]
    fn generated_catalog_rows_begin_with_zero_semantic_numerators() {
        let limits = CoverageLimits::default();
        let rows = (0..3)
            .map(|index| {
                CoverageRow::new(
                    "baseline",
                    "unit",
                    format!("row-{index}"),
                    CoverageGate::ALL,
                    limits,
                )
                .unwrap()
            })
            .collect();
        let snapshot = CoverageSnapshot::new("baseline", sha(1), 1, 3, rows, limits).unwrap();
        for gate in CoverageGate::ALL {
            assert_eq!(
                snapshot.count(gate),
                CoverageCount {
                    numerator: 0,
                    denominator: 3
                }
            );
        }
        assert_eq!(snapshot.complete_rows(), 0);
    }

    #[test]
    fn store_is_append_only_and_denominator_bound() {
        let limits = CoverageLimits::default();
        let mut store = CoverageStore::default();
        let mut row = CoverageRow::new(
            "baseline",
            "unit",
            "row",
            [CoverageGate::Recognized],
            limits,
        )
        .unwrap();
        let record = evidence("row", CoverageGate::Recognized, 1, EvidenceOutcome::Pass);
        store.append_evidence(record.clone(), limits).unwrap();
        store.append_evidence(record.clone(), limits).unwrap();
        row.record(record).unwrap();
        let first = CoverageSnapshot::new("baseline", sha(1), 1, 1, vec![row], limits).unwrap();
        store.commit(first.clone(), limits).unwrap();
        store.commit(first, limits).unwrap();
        let drift = CoverageSnapshot::new(
            "baseline",
            sha(2),
            2,
            1,
            vec![
                CoverageRow::new(
                    "baseline",
                    "unit",
                    "row",
                    [CoverageGate::Recognized],
                    limits,
                )
                .unwrap(),
            ],
            limits,
        )
        .unwrap();
        assert_eq!(
            store.commit(drift, limits),
            Err(CoverageProblem::DenominatorDrift)
        );
        assert_eq!(store.latest("baseline").unwrap().generation(), 1);
    }
}
