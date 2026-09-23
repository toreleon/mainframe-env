use super::*;

pub(super) const fn operation_tag(value: CicsPlanOperation) -> u16 {
    match value {
        CicsPlanOperation::IssueAbend => 239,
        CicsPlanOperation::GdsIssueAbend => 240,
        CicsPlanOperation::IssueConfirmation => 241,
        CicsPlanOperation::GdsIssueConfirmation => 242,
        CicsPlanOperation::IssueError => 249,
        CicsPlanOperation::GdsIssueError => 250,
        CicsPlanOperation::IssuePrepare => 253,
        CicsPlanOperation::GdsIssuePrepare => 254,
        CicsPlanOperation::GdsIssueSignal => 257,
        CicsPlanOperation::IssueSignal => 258,
        _ => panic!("unmapped ISSUE operation"),
    }
}

pub(super) fn operation_from_tag(value: u16) -> Result<CicsPlanOperation, CicsPlanCodecProblem> {
    match value {
        239 => Ok(CicsPlanOperation::IssueAbend),
        240 => Ok(CicsPlanOperation::GdsIssueAbend),
        241 => Ok(CicsPlanOperation::IssueConfirmation),
        242 => Ok(CicsPlanOperation::GdsIssueConfirmation),
        249 => Ok(CicsPlanOperation::IssueError),
        250 => Ok(CicsPlanOperation::GdsIssueError),
        253 => Ok(CicsPlanOperation::IssuePrepare),
        254 => Ok(CicsPlanOperation::GdsIssuePrepare),
        257 => Ok(CicsPlanOperation::GdsIssueSignal),
        258 => Ok(CicsPlanOperation::IssueSignal),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

pub(super) const fn operand_tag(value: CicsOperandName) -> u16 {
    match value {
        CicsOperandName::IssueConvid => 1472,
        CicsOperandName::IssueSession => 1473,
        _ => panic!("unmapped ISSUE operand"),
    }
}

pub(super) fn operand_from_tag(value: u16) -> Result<CicsOperandName, CicsPlanCodecProblem> {
    match value {
        1472 => Ok(CicsOperandName::IssueConvid),
        1473 => Ok(CicsOperandName::IssueSession),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}
