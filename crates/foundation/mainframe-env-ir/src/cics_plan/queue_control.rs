use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_write_transient_data_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Queue,
        CicsOperandName::From,
        CicsOperandName::Length,
    ]);
    !inputs.contains(&CicsOperandName::Queue)
        || !inputs.contains(&CicsOperandName::From)
        || !inputs.is_subset(&allowed_inputs)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Queue => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::From => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::Length => matches!(operand.value, CicsOperandValue::Literal(_)),
            _ => true,
        })
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}

pub(super) fn invalid_delete_transient_data_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let maximum = if plan.operation == super::CicsPlanOperation::DeleteTemporaryStorage {
        8
    } else {
        4
    };
    *inputs != BTreeSet::from([CicsOperandName::Queue])
        || match plan.operands.first().map(|operand| &operand.value) {
            Some(CicsOperandValue::Literal(value)) => {
                !(1..=maximum).contains(&value.len())
                    || !value
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            }
            Some(CicsOperandValue::Storage(_)) => false,
            _ => true,
        }
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
}
