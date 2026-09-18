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
            let allowed = BTreeSet::from([CicsOperandName::Interval, CicsOperandName::ReqId]);
            !inputs.is_subset(&allowed)
                || plan.operands.iter().any(|operand| {
                    match operand.name {
                        CicsOperandName::Interval => {
                            !matches!(operand.value, CicsOperandValue::Integer(value) if valid_hhmmss(value))
                        }
                        CicsOperandName::ReqId => !matches!(
                            operand.value,
                            CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                        ),
                        _ => true,
                    }
                })
                || inputs.contains(&CicsOperandName::ReqId)
                    && !matches!(
                        operand_value(plan, CicsOperandName::Interval),
                        Some(CicsOperandValue::Integer(value)) if *value > 0
                    )
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
                CicsOperandName::ReturnTransId,
                CicsOperandName::ReturnTermId,
                CicsOperandName::Queue,
                CicsOperandName::UserId,
            ]);
            !inputs.is_subset(&allowed)
                || !inputs.contains(&CicsOperandName::TransId)
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
                        || operand.name == CicsOperandName::UserId
                            && !matches!(
                                operand.value,
                                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                            )
                })
                || scheduling_options
                || outputs.contains(&CicsOutputName::Into)
        }
        CicsPlanOperation::Retrieve => {
            let allowed_outputs = BTreeSet::from([
                CicsOutputName::Into,
                CicsOutputName::SetPointer,
                CicsOutputName::Length,
                CicsOutputName::ReturnTransId,
                CicsOutputName::ReturnTermId,
                CicsOutputName::Queue,
                CicsOutputName::Resp,
                CicsOutputName::Resp2,
            ]);
            let into_form = outputs.contains(&CicsOutputName::Into);
            let set_form = outputs.contains(&CicsOutputName::SetPointer);
            into_form == set_form
                || if into_form {
                    *inputs != BTreeSet::from([CicsOperandName::Length])
                } else {
                    !inputs.is_empty()
                }
                || !outputs.is_subset(&allowed_outputs)
                || !outputs.contains(&CicsOutputName::Length)
                || if into_form {
                    match operand_value(plan, CicsOperandName::Length) {
                        Some(CicsOperandValue::Storage(slot)) => {
                            output_target(&plan.outputs, CicsOutputName::Length) != Some(slot)
                        }
                        Some(_) | None => true,
                    }
                } else {
                    operand_value(plan, CicsOperandName::Length).is_some()
                }
                || scheduling_options
        }
        _ => true,
    }
}

fn valid_hhmmss(value: i64) -> bool {
    (0..=995_959).contains(&value) && value / 100 % 100 <= 59 && value % 100 <= 59
}
