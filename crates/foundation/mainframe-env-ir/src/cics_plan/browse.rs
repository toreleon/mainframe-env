use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption, output_target,
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
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}
