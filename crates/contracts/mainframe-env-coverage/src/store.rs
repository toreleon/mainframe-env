use super::{CoverageLimits, CoverageProblem, CoverageSnapshot, CoverageStore, EvidenceRecord};
use std::collections::BTreeMap;

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
        let mut retained = self
            .evidence_keys
            .range((key.0.clone(), key.1, 0)..=(key.0.clone(), key.1, u64::MAX));
        if retained
            .next_back()
            .is_some_and(|((_, _, sequence), _)| *sequence >= record.sequence())
        {
            return Err(CoverageProblem::StaleEvidence);
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
        self.validate_snapshot_continuity(&snapshot)?;
        if anchor_is_new {
            self.anchors.insert(baseline.clone(), anchor);
        }
        self.snapshots
            .entry(baseline)
            .or_default()
            .insert(snapshot.generation(), snapshot);
        Ok(())
    }

    fn validate_snapshot_continuity(
        &self,
        snapshot: &CoverageSnapshot,
    ) -> Result<(), CoverageProblem> {
        if let Some(previous) = self.latest(snapshot.baseline_id()) {
            if snapshot.rows.keys().ne(previous.rows.keys()) {
                return Err(CoverageProblem::DenominatorDrift);
            }
            for (row_id, previous_row) in &previous.rows {
                let row = &snapshot.rows[row_id];
                if row.unit_id != previous_row.unit_id || row.applicable != previous_row.applicable
                {
                    return Err(CoverageProblem::DenominatorDrift);
                }
                for (gate, history) in &previous_row.evidence {
                    if !row
                        .evidence
                        .get(gate)
                        .is_some_and(|next| next.starts_with(history))
                    {
                        return Err(CoverageProblem::EvidenceConflict);
                    }
                }
            }
        }
        // Every retained observation for an admitted row/gate belongs in the new generation,
        // including observations appended before the first snapshot or after the previous one.
        for (row_id, row) in &snapshot.rows {
            for gate in &row.applicable {
                let mut history = row.evidence.get(gate).into_iter().flatten();
                for (_, identity) in self
                    .evidence_keys
                    .range((row_id.clone(), *gate, 0)..=(row_id.clone(), *gate, u64::MAX))
                {
                    let record = history.next().ok_or(CoverageProblem::MissingEvidence)?;
                    if record.identity() != identity {
                        return Err(CoverageProblem::EvidenceConflict);
                    }
                }
                if history.next().is_some() {
                    return Err(CoverageProblem::MissingEvidence);
                }
            }
        }
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
