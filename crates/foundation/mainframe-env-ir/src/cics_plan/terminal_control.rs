use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Map,
        CicsOperandName::Mapset,
        CicsOperandName::From,
    ]);
    let required = match plan.operation {
        CicsPlanOperation::ReceiveMap => BTreeSet::from([CicsOperandName::Map]),
        CicsPlanOperation::SendMap => BTreeSet::from([CicsOperandName::Map]),
        CicsPlanOperation::SendText => BTreeSet::from([CicsOperandName::From]),
        _ => return true,
    };
    !inputs.is_subset(&allowed_inputs)
        || !required.is_subset(inputs)
        || (plan.operation == CicsPlanOperation::ReceiveMap
            && inputs.contains(&CicsOperandName::From))
        || (plan.operation == CicsPlanOperation::SendText
            && inputs.iter().any(|name| *name != CicsOperandName::From))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Map | CicsOperandName::Mapset => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::From => !matches!(operand.value, CicsOperandValue::Storage(_)),
            _ => true,
        })
        || (plan.operation != CicsPlanOperation::ReceiveMap
            && outputs.contains(&CicsOutputName::Into))
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}
