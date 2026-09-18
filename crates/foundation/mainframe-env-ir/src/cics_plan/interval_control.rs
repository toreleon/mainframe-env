use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption, operand_value, output_target,
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
            let allowed = BTreeSet::from([
                CicsOperandName::Interval,
                CicsOperandName::ReqId,
                CicsOperandName::Hours,
                CicsOperandName::Minutes,
                CicsOperandName::Seconds,
            ]);
            let components = [
                CicsOperandName::Hours,
                CicsOperandName::Minutes,
                CicsOperandName::Seconds,
            ]
            .into_iter()
            .filter(|name| inputs.contains(name))
            .count();
            let explicit_modes = usize::from(plan.options.contains(&CicsPlanOption::For))
                + usize::from(plan.options.contains(&CicsPlanOption::Until));
            let schedules =
                usize::from(inputs.contains(&CicsOperandName::Interval)) + explicit_modes;
            !inputs.is_subset(&allowed)
                || schedules > 1
                || (components > 0) != (explicit_modes == 1)
                || plan.operands.iter().any(|operand| {
                    match operand.name {
                        CicsOperandName::Interval => {
                            !matches!(operand.value, CicsOperandValue::Integer(value) if valid_hhmmss(value))
                        }
                        CicsOperandName::ReqId => !matches!(
                            operand.value,
                            CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                        ),
                        CicsOperandName::Hours
                        | CicsOperandName::Minutes
                        | CicsOperandName::Seconds => !matches!(
                            operand.value,
                            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
                        ),
                        _ => true,
                    }
                })
                || inputs.contains(&CicsOperandName::ReqId)
                    && explicit_modes == 0
                    && !matches!(operand_value(plan, CicsOperandName::Interval), Some(CicsOperandValue::Integer(value)) if *value > 0)
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
                CicsOperandName::Hours,
                CicsOperandName::Minutes,
                CicsOperandName::Seconds,
            ]);
            let components = [
                CicsOperandName::Hours,
                CicsOperandName::Minutes,
                CicsOperandName::Seconds,
            ]
            .into_iter()
            .filter(|name| inputs.contains(name))
            .count();
            let explicit_modes = usize::from(plan.options.contains(&CicsPlanOption::After))
                + usize::from(plan.options.contains(&CicsPlanOption::At));
            let schedules = usize::from(inputs.contains(&CicsOperandName::Interval))
                + usize::from(inputs.contains(&CicsOperandName::StartTime))
                + explicit_modes;
            !inputs.is_subset(&allowed)
                || !inputs.contains(&CicsOperandName::TransId)
                || inputs.contains(&CicsOperandName::Length)
                    && !inputs.contains(&CicsOperandName::From)
                || plan.options.contains(&CicsPlanOption::Fmh)
                    && !inputs.contains(&CicsOperandName::From)
                || schedules > 1
                || (components > 0) != (explicit_modes == 1)
                || plan.operands.iter().any(|operand| {
                    matches!(
                        operand.name,
                        CicsOperandName::Length
                            | CicsOperandName::Interval
                            | CicsOperandName::StartTime
                            | CicsOperandName::Hours
                            | CicsOperandName::Minutes
                            | CicsOperandName::Seconds
                    ) && matches!(operand.value, CicsOperandValue::Literal(_))
                        || matches!(
                            operand.name,
                            CicsOperandName::Hours
                                | CicsOperandName::Minutes
                                | CicsOperandName::Seconds
                        ) && !matches!(
                            operand.value,
                            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
                        )
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
