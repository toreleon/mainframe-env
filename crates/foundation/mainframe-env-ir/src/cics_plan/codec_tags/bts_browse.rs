//! Append-only container browse operation tags.

use super::{CicsPlanCodecProblem, CicsPlanOperation};

pub(super) const fn operation_tag(value: CicsPlanOperation) -> u16 {
    match value {
        CicsPlanOperation::BtsEndBrowseContainer => 197,
        CicsPlanOperation::BtsGetNextContainer => 202,
        CicsPlanOperation::BtsInquireContainer => 207,
        CicsPlanOperation::BtsStartBrowseContainer => 212,
        _ => unreachable!(),
    }
}

pub(super) fn operation_from_tag(value: u16) -> Result<CicsPlanOperation, CicsPlanCodecProblem> {
    match value {
        197 => Ok(CicsPlanOperation::BtsEndBrowseContainer),
        202 => Ok(CicsPlanOperation::BtsGetNextContainer),
        207 => Ok(CicsPlanOperation::BtsInquireContainer),
        212 => Ok(CicsPlanOperation::BtsStartBrowseContainer),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}
