//! Structural legality for the source-reviewed WRITE OPERATOR plan.

use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::OperatorText,
        CicsOperandName::OperatorTextLength,
        CicsOperandName::OperatorRouteCodes,
        CicsOperandName::OperatorNumRoutes,
        CicsOperandName::OperatorConsName,
        CicsOperandName::OperatorAction,
        CicsOperandName::OperatorMaxLength,
        CicsOperandName::OperatorTimeout,
    ]);
    let action_flags = [
        CicsPlanOption::OperatorImmediate,
        CicsPlanOption::OperatorEventual,
        CicsPlanOption::OperatorCritical,
    ]
    .into_iter()
    .filter(|flag| plan.options.contains(flag))
    .count();
    let reply = outputs.contains(&CicsOutputName::OperatorReply);
    !inputs.contains(&CicsOperandName::OperatorText)
        || !inputs.is_subset(&allowed)
        || inputs.contains(&CicsOperandName::OperatorConsName)
            && inputs.contains(&CicsOperandName::OperatorRouteCodes)
        || inputs.contains(&CicsOperandName::OperatorNumRoutes)
            && !inputs.contains(&CicsOperandName::OperatorRouteCodes)
        || inputs.contains(&CicsOperandName::OperatorAction) && action_flags != 0
        || action_flags > 1
        || reply != inputs.contains(&CicsOperandName::OperatorMaxLength)
        || !reply
            && (inputs.contains(&CicsOperandName::OperatorTimeout)
                || outputs.contains(&CicsOutputName::OperatorReplyLength))
        || plan.operands.iter().any(|operand| {
            matches!(
                operand.name,
                CicsOperandName::OperatorTextLength
                    | CicsOperandName::OperatorNumRoutes
                    | CicsOperandName::OperatorAction
                    | CicsOperandName::OperatorMaxLength
                    | CicsOperandName::OperatorTimeout
            ) && !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ) && !(matches!(
                operand.name,
                CicsOperandName::OperatorTextLength | CicsOperandName::OperatorMaxLength
            ) && matches!(operand.value, CicsOperandValue::LengthOf(_)))
        })
}
