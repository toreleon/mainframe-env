use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let basic = matches!(
        plan.operation,
        CicsPlanOperation::GdsIssueAbend
            | CicsPlanOperation::GdsIssueConfirmation
            | CicsPlanOperation::GdsIssueError
            | CicsPlanOperation::GdsIssuePrepare
            | CicsPlanOperation::GdsIssueSignal
    );
    let allowed_inputs = if basic {
        BTreeSet::from([CicsOperandName::IssueConvid])
    } else {
        BTreeSet::from([CicsOperandName::IssueConvid, CicsOperandName::IssueSession])
    };
    let allowed_outputs = if basic {
        BTreeSet::from([
            CicsOutputName::IssueState,
            CicsOutputName::IssueConvData,
            CicsOutputName::IssueRetCode,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ])
    } else {
        BTreeSet::from([
            CicsOutputName::IssueState,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ])
    };
    !inputs.is_subset(&allowed_inputs)
        || !outputs.is_subset(&allowed_outputs)
        || !plan
            .options
            .is_subset(&BTreeSet::from([CicsPlanOption::NoHandle]))
        || inputs.contains(&CicsOperandName::IssueConvid)
            && inputs.contains(&CicsOperandName::IssueSession)
        || basic
            && (!inputs.contains(&CicsOperandName::IssueConvid)
                || !outputs.contains(&CicsOutputName::IssueConvData)
                || !outputs.contains(&CicsOutputName::IssueRetCode))
        || plan.operands.iter().any(|operand| {
            !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
}
