use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption,
    output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_link_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([CicsOperandName::Program, CicsOperandName::Commarea]);
    let program = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Program);
    let commarea = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Commarea);
    let commarea_output = output_target(&plan.outputs, CicsOutputName::Commarea);
    !inputs.contains(&CicsOperandName::Program)
        || !inputs.is_subset(&allowed_inputs)
        || program.is_none_or(|operand| {
            !matches!(
                &operand.value,
                CicsOperandValue::Literal(bytes) if valid_program_name(bytes)
            ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || commarea.is_some_and(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
        || match commarea.map(|operand| &operand.value) {
            Some(CicsOperandValue::Storage(slot)) => commarea_output != Some(slot),
            Some(_) => true,
            None => commarea_output.is_some(),
        }
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || outputs.contains(&CicsOutputName::Into)
}

fn valid_program_name(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 1..=8) && bytes.iter().all(u8::is_ascii_alphanumeric)
}
