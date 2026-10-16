//! Per-derivation borrowed obligation index; no process-wide state or gate expansion.
use super::{CompiledSpec, CoverageGate, MandatoryObligation, OfficialRowId};
use std::collections::BTreeMap;

pub(super) struct ObligationsByRow<'a> {
    rows: BTreeMap<&'a OfficialRowId, Vec<&'a MandatoryObligation>>,
}

impl<'a> ObligationsByRow<'a> {
    pub(super) fn new(spec: &'a CompiledSpec) -> Self {
        let mut rows: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for obligation in spec.obligations.values() {
            rows.entry(&obligation.row_id).or_default().push(obligation);
        }
        Self { rows }
    }

    pub(super) fn for_gate(
        &self,
        row_id: &OfficialRowId,
        gate: CoverageGate,
    ) -> Vec<&'a MandatoryObligation> {
        self.rows
            .get(row_id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|obligation| obligation.applicable_gates.contains(&gate))
            .collect()
    }
}
