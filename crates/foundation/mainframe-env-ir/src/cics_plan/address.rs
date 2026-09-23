use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::CommareaPointer,
        CicsOperandName::UsingAddress,
    ]);
    !inputs.contains(&CicsOperandName::CommareaPointer)
        || !inputs.is_subset(&allowed)
        || plan
            .operands
            .iter()
            .any(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}
