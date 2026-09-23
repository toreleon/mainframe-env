use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_define_input_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs.len() != 1
        || !inputs.contains(&CicsOperandName::Event)
        || plan.operands.iter().any(|operand| {
            operand.name != CicsOperandName::Event
                || !matches!(
                    operand.value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}
