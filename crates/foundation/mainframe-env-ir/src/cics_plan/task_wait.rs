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

pub(super) fn invalid_wait_external_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::EcbList,
        CicsOperandName::NumEvents,
        CicsOperandName::Purgeability,
        CicsOperandName::WaitName,
    ]);
    let purge_options = plan
        .options
        .iter()
        .filter(|option| {
            matches!(
                option,
                CicsPlanOption::Purgeable | CicsPlanOption::NotPurgeable
            )
        })
        .count();
    !inputs.contains(&CicsOperandName::EcbList)
        || !inputs.contains(&CicsOperandName::NumEvents)
        || !inputs.is_subset(&allowed)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::EcbList => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::NumEvents => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::Purgeability => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::WaitName => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::Purgeable | CicsPlanOption::NotPurgeable
            )
        })
        || purge_options > 1
        || purge_options + usize::from(inputs.contains(&CicsOperandName::Purgeability)) > 1
        || outputs.contains(&CicsOutputName::Into)
}
