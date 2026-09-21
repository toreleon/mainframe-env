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
        CicsOperandName::Length,
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
            && inputs
                .iter()
                .any(|name| !matches!(name, CicsOperandName::Map | CicsOperandName::Mapset)))
        || (plan.operation == CicsPlanOperation::SendText
            && inputs
                .iter()
                .any(|name| !matches!(name, CicsOperandName::From | CicsOperandName::Length)))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Map | CicsOperandName::Mapset => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::From => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::Length => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => true,
        })
        || match (
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::From),
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Length),
        ) {
            (None, Some(_)) => true,
            (
                Some(super::CicsNamedOperand {
                    value: CicsOperandValue::Storage(from),
                    ..
                }),
                Some(super::CicsNamedOperand {
                    value: CicsOperandValue::LengthOf(length),
                    ..
                }),
            ) => from != length,
            (Some(_), Some(_)) => false,
            (_, None) => false,
        }
        || (plan.operation != CicsPlanOperation::ReceiveMap
            && outputs.contains(&CicsOutputName::Into))
        || plan.options.iter().any(|option| match option {
            CicsPlanOption::NoHandle => false,
            CicsPlanOption::Erase => !matches!(
                plan.operation,
                CicsPlanOperation::SendMap | CicsPlanOperation::SendText
            ),
            CicsPlanOption::Cursor => plan.operation != CicsPlanOperation::SendMap,
            CicsPlanOption::FreeKb => !matches!(
                plan.operation,
                CicsPlanOperation::SendMap | CicsPlanOperation::SendText
            ),
            _ => true,
        })
}
