use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption,
    operand_value, output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    resources: usize,
) -> bool {
    resources != 1
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::File,
            CicsOperandName::Dataset,
            CicsOperandName::Ridfld,
            CicsOperandName::Length,
            CicsOperandName::KeyLength,
        ]))
        || !inputs.contains(&CicsOperandName::Ridfld)
        || inputs.contains(&CicsOperandName::From)
        || !outputs.contains(&CicsOutputName::Into)
        || (plan.options.contains(&CicsPlanOption::Generic)
            && !inputs.contains(&CicsOperandName::KeyLength))
        || (plan.options.contains(&CicsPlanOption::Equal)
            && plan.options.contains(&CicsPlanOption::Gteq))
        || plan.operands.iter().any(|operand| {
            operand.name == CicsOperandName::KeyLength
                && match operand.value {
                    CicsOperandValue::Integer(0) => !plan.options.contains(&CicsPlanOption::Gteq),
                    CicsOperandValue::Integer(1..=32_767)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_) => false,
                    _ => true,
                }
        })
        || match (
            operand_value(plan, CicsOperandName::Ridfld),
            operand_value(plan, CicsOperandName::KeyLength),
        ) {
            (Some(CicsOperandValue::Storage(ridfld)), Some(CicsOperandValue::LengthOf(length))) => {
                ridfld != length
            }
            (Some(_), Some(CicsOperandValue::LengthOf(_))) => true,
            _ => false,
        }
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::Generic
                    | CicsPlanOption::Gteq
                    | CicsPlanOption::Equal
                    | CicsPlanOption::NoHandle
                    | CicsPlanOption::Update
            )
        })
        || !matches!(
            operand_value(plan, CicsOperandName::Length),
            None | Some(CicsOperandValue::Storage(_) | CicsOperandValue::LengthOf(_))
        )
        || outputs.contains(&CicsOutputName::Length)
            != matches!(
                operand_value(plan, CicsOperandName::Length),
                Some(CicsOperandValue::Storage(_))
            )
        || match operand_value(plan, CicsOperandName::Length) {
            Some(CicsOperandValue::Storage(slot)) => {
                output_target(&plan.outputs, CicsOutputName::Length) != Some(slot)
            }
            Some(CicsOperandValue::LengthOf(slot)) => {
                output_target(&plan.outputs, CicsOutputName::Into) != Some(slot)
            }
            _ => false,
        }
}
