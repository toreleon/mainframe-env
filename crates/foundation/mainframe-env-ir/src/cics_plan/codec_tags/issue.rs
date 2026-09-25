use super::*;

pub(super) const fn operation_tag(value: CicsPlanOperation) -> u16 {
    match value {
        CicsPlanOperation::IssueAbend => 239,
        CicsPlanOperation::GdsIssueAbend => 240,
        CicsPlanOperation::IssueConfirmation => 241,
        CicsPlanOperation::GdsIssueConfirmation => 242,
        CicsPlanOperation::IssueCopy => 243,
        CicsPlanOperation::IssueDisconnect => 244,
        CicsPlanOperation::IssueEndfile => 245,
        CicsPlanOperation::IssueEndoutput => 246,
        CicsPlanOperation::IssueEods => 247,
        CicsPlanOperation::IssueEraseAup => 248,
        CicsPlanOperation::IssueError => 249,
        CicsPlanOperation::GdsIssueError => 250,
        CicsPlanOperation::IssueLoad => 251,
        CicsPlanOperation::IssuePass => 252,
        CicsPlanOperation::IssuePrepare => 253,
        CicsPlanOperation::GdsIssuePrepare => 254,
        CicsPlanOperation::IssuePrint => 255,
        CicsPlanOperation::IssueReset => 256,
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
        243 => Ok(CicsPlanOperation::IssueCopy),
        244 => Ok(CicsPlanOperation::IssueDisconnect),
        245 => Ok(CicsPlanOperation::IssueEndfile),
        246 => Ok(CicsPlanOperation::IssueEndoutput),
        247 => Ok(CicsPlanOperation::IssueEods),
        248 => Ok(CicsPlanOperation::IssueEraseAup),
        249 => Ok(CicsPlanOperation::IssueError),
        250 => Ok(CicsPlanOperation::GdsIssueError),
        251 => Ok(CicsPlanOperation::IssueLoad),
        252 => Ok(CicsPlanOperation::IssuePass),
        253 => Ok(CicsPlanOperation::IssuePrepare),
        254 => Ok(CicsPlanOperation::GdsIssuePrepare),
        255 => Ok(CicsPlanOperation::IssuePrint),
        256 => Ok(CicsPlanOperation::IssueReset),
        257 => Ok(CicsPlanOperation::GdsIssueSignal),
        258 => Ok(CicsPlanOperation::IssueSignal),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

pub(super) const fn operand_tag(value: CicsOperandName) -> u16 {
    match value {
        CicsOperandName::IssueConvid => 1472,
        CicsOperandName::IssueSession => 1473,
        CicsOperandName::IssueTermId => 1474,
        CicsOperandName::IssueCtlChar => 1475,
        CicsOperandName::IssueProgram => 1476,
        CicsOperandName::IssueLuName => 1477,
        CicsOperandName::IssueFrom => 1478,
        CicsOperandName::IssueLength => 1479,
        CicsOperandName::IssueLogMode => 1480,
        _ => panic!("unmapped ISSUE operand"),
    }
}

pub(super) fn operand_from_tag(value: u16) -> Result<CicsOperandName, CicsPlanCodecProblem> {
    match value {
        1472 => Ok(CicsOperandName::IssueConvid),
        1473 => Ok(CicsOperandName::IssueSession),
        1474 => Ok(CicsOperandName::IssueTermId),
        1475 => Ok(CicsOperandName::IssueCtlChar),
        1476 => Ok(CicsOperandName::IssueProgram),
        1477 => Ok(CicsOperandName::IssueLuName),
        1478 => Ok(CicsOperandName::IssueFrom),
        1479 => Ok(CicsOperandName::IssueLength),
        1480 => Ok(CicsOperandName::IssueLogMode),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

pub(super) const fn option_tag(value: CicsPlanOption) -> u16 {
    match value {
        CicsPlanOption::IssueWaitOption => 1404,
        CicsPlanOption::IssueEndOutput => 1405,
        CicsPlanOption::IssueEndFile => 1406,
        CicsPlanOption::IssueConverse => 1407,
        CicsPlanOption::IssueLogonLogmode => 1408,
        CicsPlanOption::IssueNoQuiesce => 1409,
        _ => panic!("unmapped ISSUE option"),
    }
}

pub(super) fn option_from_tag(value: u16) -> Result<CicsPlanOption, CicsPlanCodecProblem> {
    match value {
        1404 => Ok(CicsPlanOption::IssueWaitOption),
        1405 => Ok(CicsPlanOption::IssueEndOutput),
        1406 => Ok(CicsPlanOption::IssueEndFile),
        1407 => Ok(CicsPlanOption::IssueConverse),
        1408 => Ok(CicsPlanOption::IssueLogonLogmode),
        1409 => Ok(CicsPlanOption::IssueNoQuiesce),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}
