use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption, operand_value, output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let resources = usize::from(inputs.contains(&CicsOperandName::File))
        + usize::from(inputs.contains(&CicsOperandName::Dataset));
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::File,
        CicsOperandName::Dataset,
        CicsOperandName::Ridfld,
        CicsOperandName::Length,
        CicsOperandName::KeyLength,
    ]);
    let ridfld = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Ridfld);
    let ridfld_output = output_target(&plan.outputs, CicsOutputName::Ridfld);
    let reading = matches!(
        plan.operation,
        CicsPlanOperation::ReadNext | CicsPlanOperation::ReadPrev
    );
    let starting = plan.operation == CicsPlanOperation::StartBrowse;
    let generic = plan.options.contains(&CicsPlanOption::Generic);
    let positions = matches!(plan.operation, CicsPlanOperation::StartBrowse) || reading;
    resources != 1
        || !inputs.is_subset(&allowed_inputs)
        || positions != inputs.contains(&CicsOperandName::Ridfld)
        || ridfld.is_some_and(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
        || match ridfld.map(|operand| &operand.value) {
            Some(CicsOperandValue::Storage(slot)) if reading => ridfld_output != Some(slot),
            Some(CicsOperandValue::Storage(_)) => ridfld_output.is_some(),
            Some(_) => true,
            None => ridfld_output.is_some(),
        }
        || reading != outputs.contains(&CicsOutputName::Into)
        || (generic && (!starting || !inputs.contains(&CicsOperandName::KeyLength)))
        || (plan.options.contains(&CicsPlanOption::Equal)
            && plan.options.contains(&CicsPlanOption::Gteq))
        || (starting
            && plan.operands.iter().any(|operand| {
                operand.name == CicsOperandName::KeyLength
                    && match operand.value {
                        CicsOperandValue::Integer(0) => {
                            !plan.options.contains(&CicsPlanOption::Gteq)
                        }
                        CicsOperandValue::Integer(1..=32_767)
                        | CicsOperandValue::Storage(_)
                        | CicsOperandValue::LengthOf(_) => false,
                        _ => true,
                    }
            }))
        || (starting
            && match (
                operand_value(plan, CicsOperandName::Ridfld),
                operand_value(plan, CicsOperandName::KeyLength),
            ) {
                (
                    Some(CicsOperandValue::Storage(ridfld)),
                    Some(CicsOperandValue::LengthOf(length)),
                ) => ridfld != length,
                (Some(_), Some(CicsOperandValue::LengthOf(_))) => true,
                _ => false,
            })
        || plan.options.iter().any(|option| match option {
            CicsPlanOption::NoHandle => false,
            CicsPlanOption::Gteq | CicsPlanOption::Equal | CicsPlanOption::Generic => {
                plan.operation != CicsPlanOperation::StartBrowse
            }
            _ => true,
        })
}
