//! Immutable six-gate official coverage contracts.

#![forbid(unsafe_code)]

mod candidate;
mod conformance;
mod gate;
mod store;

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

    fn continuity_row(records: &[EvidenceRecord]) -> CoverageRow {
        let mut row = CoverageRow::new(
            "baseline",
            "unit",
            "row",
            CoverageGate::ALL,
            CoverageLimits::default(),
        )
        .unwrap();
        for record in records {
            row.record(record.clone()).unwrap();
        }
        row
    }

    fn continuity_snapshot(generation: u64, row: CoverageRow) -> CoverageSnapshot {
        CoverageSnapshot::new(
            "baseline",
            sha(1),
            generation,
            1,
            vec![row],
            CoverageLimits::default(),
        )
        .unwrap()
    }

    fn assert_continuity_unchanged(store: &CoverageStore, before: &CoverageStore) {
        assert_eq!(store.anchors, before.anchors);
        assert_eq!(store.evidence, before.evidence);
        assert_eq!(store.evidence_keys, before.evidence_keys);
        assert_eq!(store.snapshots, before.snapshots);
    }

    #[test]
    fn snapshot_continuity_rejects_same_count_descriptor_drift() {
        let limits = CoverageLimits::default();
        let mut store = CoverageStore::default();
        store
            .commit(continuity_snapshot(1, continuity_row(&[])), limits)
            .unwrap();
        for (unit, row_id, gates) in [
            ("unit", "replacement", CoverageGate::ALL.to_vec()),
            ("replacement", "row", CoverageGate::ALL.to_vec()),
            ("unit", "row", vec![CoverageGate::Recognized]),
            (
                "unit",
                "row",
                CoverageGate::ALL
                    .into_iter()
                    .filter(|gate| *gate != CoverageGate::Differential)
                    .collect(),
            ),
        ] {
            let before = store.clone();
            let row = CoverageRow::new("baseline", unit, row_id, gates, limits).unwrap();
            assert_eq!(
                store.commit(continuity_snapshot(2, row), limits),
                Err(CoverageProblem::DenominatorDrift),
                "descriptor drift: {unit}/{row_id}"
            );
            assert_continuity_unchanged(&store, &before);
        }
    }

    #[test]
    fn snapshot_continuity_rejects_changed_or_extended_applicability() {
        let limits = CoverageLimits::default();
        let mut store = CoverageStore::default();
        let first = CoverageRow::new(
            "baseline",
            "unit",
            "row",
            [CoverageGate::Recognized],
            limits,
        )
        .unwrap();
        store.commit(continuity_snapshot(1, first), limits).unwrap();
        for gates in [
            vec![CoverageGate::Validated],
            vec![CoverageGate::Recognized, CoverageGate::Validated],
        ] {
            let before = store.clone();
            let row = CoverageRow::new("baseline", "unit", "row", gates, limits).unwrap();
            assert_eq!(
                store.commit(continuity_snapshot(2, row), limits),
                Err(CoverageProblem::DenominatorDrift)
            );
            assert_continuity_unchanged(&store, &before);
        }
    }

    #[test]
    fn snapshot_continuity_rejects_dropped_committed_history() {
        let limits = CoverageLimits::default();
        let pass = evidence("row", CoverageGate::Recognized, 1, EvidenceOutcome::Pass);
        let fail = evidence("row", CoverageGate::Recognized, 2, EvidenceOutcome::Fail);
        let mut store = CoverageStore::default();
        for record in [&pass, &fail] {
            store.append_evidence(record.clone(), limits).unwrap();
        }
        store
            .commit(
                continuity_snapshot(1, continuity_row(&[pass.clone(), fail.clone()])),
                limits,
            )
            .unwrap();
        for records in [vec![], vec![pass], vec![fail]] {
            let before = store.clone();
            assert_eq!(
                store.commit(continuity_snapshot(2, continuity_row(&records)), limits),
                Err(CoverageProblem::EvidenceConflict)
            );
            assert_continuity_unchanged(&store, &before);
        }
    }

    #[test]
    fn snapshot_continuity_cannot_hide_retained_failure_before_first_or_next_commit() {
        let limits = CoverageLimits::default();
        let pass = evidence("row", CoverageGate::Recognized, 1, EvidenceOutcome::Pass);
        let fail = evidence("row", CoverageGate::Recognized, 2, EvidenceOutcome::Fail);
        for previous_generation in [false, true] {
            let mut store = CoverageStore::default();
            store.append_evidence(pass.clone(), limits).unwrap();
            let passing = continuity_snapshot(1, continuity_row(std::slice::from_ref(&pass)));
            if previous_generation {
                store.commit(passing.clone(), limits).unwrap();
            }
            store.append_evidence(fail.clone(), limits).unwrap();
            if previous_generation {
                // A retry acknowledges the immutable old generation, not a new observation.
                store.commit(passing, limits).unwrap();
            }
            let before = store.clone();
            let generation = if previous_generation { 2 } else { 1 };
            assert_eq!(
                store.commit(
                    continuity_snapshot(generation, continuity_row(std::slice::from_ref(&pass))),
                    limits,
                ),
                Err(CoverageProblem::MissingEvidence)
            );
            assert_continuity_unchanged(&store, &before);
            let accepted =
                continuity_snapshot(generation, continuity_row(&[pass.clone(), fail.clone()]));
            store.commit(accepted, limits).unwrap();
            let latest = store.latest("baseline").unwrap();
            assert_eq!(latest.count(CoverageGate::Recognized).numerator, 0);
            assert_eq!(latest.complete_rows(), 0);
        }
    }

    #[test]
    fn snapshot_continuity_rejects_reordered_history_and_unstored_replacement() {
        let limits = CoverageLimits::default();
        let pass = evidence("row", CoverageGate::Recognized, 1, EvidenceOutcome::Pass);
        let fail = evidence("row", CoverageGate::Recognized, 2, EvidenceOutcome::Fail);
        let mut store = CoverageStore::default();
        for record in [&pass, &fail] {
            store.append_evidence(record.clone(), limits).unwrap();
        }
        let before = store.clone();
        // Exercise admission against malformed retained history, bypassing the row builder.
        let mut reordered = continuity_row(&[pass.clone(), fail.clone()]);
        reordered
            .evidence
            .get_mut(&CoverageGate::Recognized)
            .unwrap()
            .reverse();
        assert_eq!(
            store.commit(continuity_snapshot(1, reordered), limits),
            Err(CoverageProblem::EvidenceConflict)
        );
        assert_continuity_unchanged(&store, &before);
        let replacement = evidence("row", CoverageGate::Recognized, 2, EvidenceOutcome::Pass);
        assert_eq!(
            store.commit(
                continuity_snapshot(1, continuity_row(&[pass, replacement])),
                limits,
            ),
            Err(CoverageProblem::MissingEvidence)
        );
        assert_continuity_unchanged(&store, &before);
    }

    #[test]
    fn snapshot_continuity_rejects_out_of_order_retention_without_mutation() {
        let limits = CoverageLimits::default();
        let fail = evidence("row", CoverageGate::Recognized, 2, EvidenceOutcome::Fail);
        let pass = evidence("row", CoverageGate::Recognized, 1, EvidenceOutcome::Pass);
        let mut store = CoverageStore::default();
        store.append_evidence(fail.clone(), limits).unwrap();
        let before = store.clone();
        assert_eq!(
            store.append_evidence(pass, limits),
            Err(CoverageProblem::StaleEvidence)
        );
        assert_continuity_unchanged(&store, &before);
        store.append_evidence(fail, limits).unwrap();
        assert_continuity_unchanged(&store, &before);
    }

    #[test]
    fn snapshot_continuity_accepts_monotonic_append_and_historical_retry() {
        let limits = CoverageLimits::default();
        let mut store = CoverageStore::default();
        let mut row = continuity_row(&[]);
        let first = continuity_snapshot(1, row.clone());
        store.commit(first.clone(), limits).unwrap();
        for (generation, outcome) in [
            (2, EvidenceOutcome::Pass),
            (3, EvidenceOutcome::Fail),
            (4, EvidenceOutcome::Pass),
        ] {
            let record = evidence("row", CoverageGate::Recognized, generation, outcome);
            store.append_evidence(record.clone(), limits).unwrap();
            row.record(record).unwrap();
            let next = continuity_snapshot(generation, row.clone());
            store.commit(next.clone(), limits).unwrap();
            store.commit(next, limits).unwrap();
        }
        let before = store.clone();
        store.commit(first, limits).unwrap();
        assert_continuity_unchanged(&store, &before);
        let latest = store.latest("baseline").unwrap();
        assert_eq!(latest.generation(), 4);
        assert_eq!(latest.evidence().count(), 3);
        assert_eq!(latest.count(CoverageGate::Recognized).numerator, 1);
        assert_eq!(latest.count(CoverageGate::Differential).numerator, 0);
        assert_eq!(latest.complete_rows(), 0);
    }

    #[test]
    fn snapshot_continuity_closure_is_scoped_to_admitted_rows_and_gates() {
        let limits = CoverageLimits::default();
        let mut store = CoverageStore::default();
        for (row, gate) in [
            ("other-row", CoverageGate::Recognized),
            ("row", CoverageGate::Executed),
        ] {
            store
                .append_evidence(evidence(row, gate, 1, EvidenceOutcome::Fail), limits)
                .unwrap();
        }
        let row = CoverageRow::new(
            "baseline",
            "unit",
            "row",
            [CoverageGate::Recognized, CoverageGate::Validated],
            limits,
        )
        .unwrap();
        store.commit(continuity_snapshot(1, row), limits).unwrap();
        let latest = store.latest("baseline").unwrap();
        assert_eq!(latest.count(CoverageGate::Recognized).numerator, 0);
        assert_eq!(latest.count(CoverageGate::Validated).denominator, 1);
    }
}
