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

pub(super) fn invalid_define_composite_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let all = plan.options.contains(&CicsPlanOption::EventAnd);
    let any = plan.options.contains(&CicsPlanOption::EventOr);
    !inputs.contains(&CicsOperandName::Event)
        || inputs.len() > 9
        || all == any
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::EventAnd | CicsPlanOption::EventOr
            )
        })
        || plan.operands.iter().any(|operand| {
            !matches!(
                operand.name,
                CicsOperandName::Event
                    | CicsOperandName::SubEvent1
                    | CicsOperandName::SubEvent2
                    | CicsOperandName::SubEvent3
                    | CicsOperandName::SubEvent4
                    | CicsOperandName::SubEvent5
                    | CicsOperandName::SubEvent6
                    | CicsOperandName::SubEvent7
                    | CicsOperandName::SubEvent8
            ) || !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}
