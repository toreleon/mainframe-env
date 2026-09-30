//! Shared canonical-order and bounded-count checks for tag codecs.

use super::CicsPlanCodecProblem;

pub(in crate::cics_plan) fn bounded_count(
    value: usize,
    maximum: usize,
) -> Result<(), CicsPlanCodecProblem> {
    if value > maximum || u32::try_from(value).is_err() {
        Err(CicsPlanCodecProblem::LimitExceeded)
    } else {
        Ok(())
    }
}

pub(in crate::cics_plan) fn require_order<T: Copy + Ord>(
    previous: Option<T>,
    current: T,
) -> Result<(), CicsPlanCodecProblem> {
    match previous {
        Some(previous) if previous == current => Err(CicsPlanCodecProblem::Malformed),
        Some(previous) if previous > current => Err(CicsPlanCodecProblem::NonCanonical),
        _ => Ok(()),
    }
}
