use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let schedule_count = usize::from(inputs.contains(&CicsOperandName::Interval))
        + usize::from(inputs.contains(&CicsOperandName::StartTime))
        + usize::from(plan.options.contains(&CicsPlanOption::After))
        + usize::from(plan.options.contains(&CicsPlanOption::At));
    let explicit_units = [
        CicsOperandName::Hours,
        CicsOperandName::Minutes,
        CicsOperandName::Seconds,
    ]
    .into_iter()
    .any(|name| inputs.contains(&name));
    let explicit_mode =
        plan.options.contains(&CicsPlanOption::After) || plan.options.contains(&CicsPlanOption::At);
    let allowed = BTreeSet::from([
        CicsOperandName::Interval,
        CicsOperandName::StartTime,
        CicsOperandName::Hours,
        CicsOperandName::Minutes,
        CicsOperandName::Seconds,
        CicsOperandName::ReqId,
    ]);
    !outputs.contains(&CicsOutputName::SetPointer)
        || !inputs.is_subset(&allowed)
        || schedule_count > 1
        || explicit_mode != explicit_units
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::ReqId => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
        })
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::After | CicsPlanOption::At | CicsPlanOption::NoHandle
            )
        })
}
