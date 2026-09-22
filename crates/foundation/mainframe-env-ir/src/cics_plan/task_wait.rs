use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_wait_event_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::EventControlAddress,
        CicsOperandName::WaitName,
    ]);
    !inputs.contains(&CicsOperandName::EventControlAddress)
        || !inputs.is_subset(&allowed)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::EventControlAddress => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::WaitName => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || outputs.contains(&CicsOutputName::Into)
}
