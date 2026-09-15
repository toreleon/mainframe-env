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
    let resources = usize::from(inputs.contains(&CicsOperandName::File))
        + usize::from(inputs.contains(&CicsOperandName::Dataset));
    let writing = plan.operation == CicsPlanOperation::Write;
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::File,
        CicsOperandName::Dataset,
        CicsOperandName::From,
        CicsOperandName::Ridfld,
        CicsOperandName::Length,
        CicsOperandName::KeyLength,
    ]);
    resources != 1
        || !inputs.is_subset(&allowed_inputs)
        || (writing && !inputs.contains(&CicsOperandName::Ridfld))
        || writing != inputs.contains(&CicsOperandName::From)
        || plan.operands.iter().any(|operand| {
            matches!(
                operand.name,
                CicsOperandName::From | CicsOperandName::Ridfld
            ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}
