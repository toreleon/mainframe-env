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
        CicsOperandName::SysId,
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
            CicsOperandName::SysId => invalid_system_value(&operand.value),
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

pub(super) fn invalid_read_transient_data_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Queue,
        CicsOperandName::Length,
        CicsOperandName::SysId,
    ]);
    let data_outputs = usize::from(outputs.contains(&CicsOutputName::Into))
        + usize::from(outputs.contains(&CicsOutputName::SetPointer));
    !inputs.contains(&CicsOperandName::Queue)
        || !inputs.is_subset(&allowed_inputs)
        || data_outputs != 1
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Queue => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::Length => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::SysId => invalid_system_value(&operand.value),
            _ => true,
        })
        || outputs.contains(&CicsOutputName::Length) != inputs.contains(&CicsOperandName::Length)
        || match plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::Length)
            .map(|operand| &operand.value)
        {
            Some(CicsOperandValue::Storage(slot)) => plan
                .outputs
                .iter()
                .find(|output| output.name == CicsOutputName::Length)
                .is_none_or(|output| output.target != *slot),
            Some(_) => true,
            None => false,
        }
        || outputs.iter().any(|output| {
            !matches!(
                output,
                CicsOutputName::Into
                    | CicsOutputName::SetPointer
                    | CicsOutputName::Length
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
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
    let temporary = plan.operation == super::CicsPlanOperation::DeleteTemporaryStorage;
    let identity = if temporary && inputs.contains(&CicsOperandName::Qname) {
        CicsOperandName::Qname
    } else {
        CicsOperandName::Queue
    };
    let maximum = if identity == CicsOperandName::Qname {
        16
    } else if temporary {
        8
    } else {
        4
    };
    let mut expected = BTreeSet::from([identity]);
    if inputs.contains(&CicsOperandName::SysId) {
        expected.insert(CicsOperandName::SysId);
    }
    *inputs != expected
        || match plan
            .operands
            .iter()
            .find(|operand| operand.name == identity)
            .map(|operand| &operand.value)
        {
            Some(CicsOperandValue::Literal(value)) => {
                !(1..=maximum).contains(&value.len())
                    || !value
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            }
            Some(CicsOperandValue::Storage(_)) => false,
            _ => true,
        }
        || plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::SysId)
            .is_some_and(|operand| match &operand.value {
                CicsOperandValue::Literal(value) => {
                    !matches!(value.len(), 1..=4) || !value.iter().all(u8::is_ascii_alphanumeric)
                }
                CicsOperandValue::Storage(_) => false,
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

fn invalid_system_value(value: &CicsOperandValue) -> bool {
    match value {
        CicsOperandValue::Literal(value) => {
            !matches!(value.len(), 1..=4) || !value.iter().all(u8::is_ascii_alphanumeric)
        }
        CicsOperandValue::Storage(_) => false,
        _ => true,
    }
}

pub(super) fn invalid_read_temporary_storage_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let identity = if inputs.contains(&CicsOperandName::Qname) {
        CicsOperandName::Qname
    } else {
        CicsOperandName::Queue
    };
    let maximum = if identity == CicsOperandName::Qname {
        16
    } else {
        8
    };
    let mut allowed_inputs = BTreeSet::from([identity, CicsOperandName::Length]);
    allowed_inputs.extend([CicsOperandName::Item, CicsOperandName::SysId]);
    let destination_count = usize::from(outputs.contains(&CicsOutputName::Into))
        + usize::from(outputs.contains(&CicsOutputName::SetPointer));
    let allowed_outputs = BTreeSet::from([
        CicsOutputName::Into,
        CicsOutputName::SetPointer,
        CicsOutputName::Length,
        CicsOutputName::NumItems,
        CicsOutputName::Resp,
        CicsOutputName::Resp2,
    ]);
    inputs.contains(&CicsOperandName::Queue) == inputs.contains(&CicsOperandName::Qname)
        || !inputs.contains(&CicsOperandName::Length)
        || !inputs.is_subset(&allowed_inputs)
        || destination_count != 1
        || !outputs.is_subset(&allowed_outputs)
        || outputs.contains(&CicsOutputName::SetPointer)
            && !outputs.contains(&CicsOutputName::Length)
        || inputs.contains(&CicsOperandName::Item) && plan.options.contains(&CicsPlanOption::Next)
        || match plan
            .operands
            .iter()
            .find(|operand| operand.name == identity)
            .map(|operand| &operand.value)
        {
            Some(CicsOperandValue::Literal(value)) => {
                !(1..=maximum).contains(&value.len())
                    || !value
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            }
            Some(CicsOperandValue::Storage(_)) => false,
            _ => true,
        }
        || match plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::Length)
            .map(|operand| &operand.value)
        {
            Some(CicsOperandValue::Storage(slot)) => plan
                .outputs
                .iter()
                .find(|output| output.name == CicsOutputName::Length)
                .is_none_or(|output| &output.target != slot),
            Some(CicsOperandValue::LengthOf(_)) => outputs.contains(&CicsOutputName::Length),
            _ => true,
        }
        || plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::Item)
            .is_some_and(|operand| {
                !matches!(
                    operand.value,
                    CicsOperandValue::Integer(-32_768..=32_767) | CicsOperandValue::Storage(_)
                )
            })
        || plan
            .operands
            .iter()
            .find(|operand| operand.name == CicsOperandName::SysId)
            .is_some_and(|operand| match &operand.value {
                CicsOperandValue::Literal(value) => {
                    !matches!(value.len(), 1..=4) || !value.iter().all(u8::is_ascii_alphanumeric)
                }
                CicsOperandValue::Storage(_) => false,
                _ => true,
            })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::Next | CicsPlanOption::NoHandle))
}
