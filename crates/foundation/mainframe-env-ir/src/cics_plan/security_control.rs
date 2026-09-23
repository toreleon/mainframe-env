use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_query_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::ResId)
        || inputs.contains(&CicsOperandName::ResClass) == inputs.contains(&CicsOperandName::ResType)
        || inputs.contains(&CicsOperandName::ResIdLength)
            != inputs.contains(&CicsOperandName::ResClass)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::ResId,
            CicsOperandName::ResClass,
            CicsOperandName::ResType,
            CicsOperandName::ResIdLength,
            CicsOperandName::LogMessage,
            CicsOperandName::UserId,
        ]))
        || !outputs.iter().any(|name| {
            matches!(
                name,
                CicsOutputName::SecurityRead
                    | CicsOutputName::SecurityUpdate
                    | CicsOutputName::SecurityControl
                    | CicsOutputName::SecurityAlter
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::ResIdLength | CicsOperandName::LogMessage => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}
