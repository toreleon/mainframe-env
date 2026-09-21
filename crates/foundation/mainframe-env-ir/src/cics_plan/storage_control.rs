use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_getmain_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::Flength)
        || inputs
            .iter()
            .any(|name| !matches!(name, CicsOperandName::Flength | CicsOperandName::InitImage))
        || !outputs.contains(&CicsOutputName::SetPointer)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::SetPointer,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Flength => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::InitImage => !matches!(operand.value, CicsOperandValue::Storage(_)),
            _ => true,
        })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::NoSuspend))
}

pub(super) fn invalid_freemain_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let pointer_form = *inputs == BTreeSet::from([CicsOperandName::DataPointer]);
    let data_form = *inputs == BTreeSet::from([CicsOperandName::DataArea]);
    (!pointer_form && !data_form)
        || !matches!(
            plan.operands.first().map(|operand| &operand.value),
            Some(CicsOperandValue::Storage(_))
        )
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}
