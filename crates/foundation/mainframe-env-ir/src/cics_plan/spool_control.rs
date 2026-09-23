use super::{
    CicsCondition, CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) fn invalid_close_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs != &BTreeSet::from([CicsOperandName::SpoolToken])
        || plan.operands.iter().any(|operand| {
            operand.name != CicsOperandName::SpoolToken
                || !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || usize::from(plan.options.contains(&CicsPlanOption::SpoolKeep))
            + usize::from(plan.options.contains(&CicsPlanOption::SpoolDelete))
            > 1
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::SpoolKeep | CicsPlanOption::SpoolDelete
            )
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
        || matches!(plan.condition, CicsCondition::Default)
}
