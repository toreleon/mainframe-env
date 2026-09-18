use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    operand_value, output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    scheduling_options: bool,
) -> bool {
    match plan.operation {
        CicsPlanOperation::Cancel => {
            let allowed = BTreeSet::from([CicsOperandName::ReqId, CicsOperandName::TransId]);
            !inputs.is_subset(&allowed)
                || !inputs.contains(&CicsOperandName::ReqId)
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Delay => {
            let allowed = BTreeSet::from([CicsOperandName::Interval]);
            !inputs.is_subset(&allowed)
                || plan.operands.iter().any(|operand| {
                    operand.name != CicsOperandName::Interval
                        || operand.value != CicsOperandValue::Integer(0)
                })
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Start => {
            let allowed = BTreeSet::from([
                CicsOperandName::TransId,
                CicsOperandName::ReqId,
                CicsOperandName::From,
                CicsOperandName::Length,
                CicsOperandName::Interval,
                CicsOperandName::StartTime,
            ]);
            !inputs.is_subset(&allowed)
                || !inputs.contains(&CicsOperandName::TransId)
                || !inputs.contains(&CicsOperandName::ReqId)
                || !inputs.contains(&CicsOperandName::From)
                || inputs.contains(&CicsOperandName::Interval)
                    && inputs.contains(&CicsOperandName::StartTime)
                || plan.operands.iter().any(|operand| {
                    matches!(
                        operand.name,
                        CicsOperandName::Length
                            | CicsOperandName::Interval
                            | CicsOperandName::StartTime
                    ) && matches!(operand.value, CicsOperandValue::Literal(_))
                })
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Retrieve => {
            *inputs != BTreeSet::from([CicsOperandName::Length])
                || !outputs.contains(&CicsOutputName::Into)
                || !outputs.contains(&CicsOutputName::Length)
                || match operand_value(plan, CicsOperandName::Length) {
                    Some(CicsOperandValue::Storage(slot)) => {
                        output_target(&plan.outputs, CicsOutputName::Length) != Some(slot)
                    }
                    Some(_) | None => true,
                }
                || scheduling_options
        }
        _ => true,
    }
}
