//! Dataset reply completeness and record-area delivery.
use super::{Decimal, HostResult, MachineProblem, PendingKind, ReferenceMachine};

pub(super) fn complete_whole_input(
    kind: &PendingKind,
    outcome: HostResult,
) -> Result<HostResult, MachineProblem> {
    match (kind, outcome) {
        (PendingKind::SortRead, HostResult::Dataset(result)) => result
            .require_complete()
            .map(HostResult::Dataset)
            .map_err(MachineProblem::Host),
        (_, outcome) => Ok(outcome),
    }
}

impl ReferenceMachine {
    pub(super) fn finish_dataset_read(
        &mut self,
        target: &str,
        into: Option<&str>,
        depending_on: Option<&str>,
        record: &[u8],
    ) -> Result<(), MachineProblem> {
        self.write(target, record)?;
        if let Some(depending_on) = depending_on {
            self.write_decimal(
                depending_on,
                Decimal {
                    coefficient: record.len() as i128,
                    scale: 0,
                },
            )?;
        }
        if let Some(into) = into {
            self.write(into, record)?;
        }
        Ok(())
    }
}
