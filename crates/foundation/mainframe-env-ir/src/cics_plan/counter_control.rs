use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) fn invalid_define_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::CounterName,
        CicsOperandName::CounterPool,
        CicsOperandName::CounterValue,
        CicsOperandName::CounterMinimum,
        CicsOperandName::CounterMaximum,
    ]);
    !inputs.contains(&CicsOperandName::CounterName)
        || !inputs.is_subset(&allowed)
        || inputs.contains(&CicsOperandName::CounterMinimum)
            && !inputs.contains(&CicsOperandName::CounterValue)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::CounterNoSuspend
            )
        })
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::CounterName => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_name(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterPool => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_pool(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterValue
            | CicsOperandName::CounterMinimum
            | CicsOperandName::CounterMaximum => match &operand.value {
                CicsOperandValue::Integer(value) => {
                    *value < 0
                        || plan.operation == CicsPlanOperation::DefineCounter
                            && *value > i64::from(i32::MAX)
                }
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            _ => true,
        })
}

fn invalid_name(bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii_end();
    trimmed.is_empty()
        || bytes.len() > 16
        || matches!(trimmed[0], b'0'..=b'9' | b'_')
        || !trimmed.iter().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"_$#@".contains(byte)
        })
}

fn invalid_pool(bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii_end();
    bytes.len() > 8
        || !trimmed.iter().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"_$#@".contains(byte)
        })
}

pub(super) fn invalid_get_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::CounterName)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::CounterName,
            CicsOperandName::CounterPool,
            CicsOperandName::CounterIncrement,
            CicsOperandName::CounterCompareMin,
            CicsOperandName::CounterCompareMax,
        ]))
        || !outputs.contains(&CicsOutputName::CounterValue)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::CounterValue,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle
                    | CicsPlanOption::CounterNoSuspend
                    | CicsPlanOption::CounterReduce
                    | CicsPlanOption::CounterWrap
            )
        })
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::CounterName => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_name(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterPool => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_pool(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterIncrement
            | CicsOperandName::CounterCompareMin
            | CicsOperandName::CounterCompareMax => match &operand.value {
                CicsOperandValue::Integer(value) => {
                    *value < 0
                        || plan.operation == CicsPlanOperation::GetCounter
                            && *value > i64::from(i32::MAX)
                }
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            _ => true,
        })
}

pub(super) fn invalid_query_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::CounterName)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::CounterName,
            CicsOperandName::CounterPool,
        ]))
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::CounterValue,
            CicsOutputName::CounterMinimum,
            CicsOutputName::CounterMaximum,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::CounterNoSuspend
            )
        })
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::CounterName => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_name(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterPool => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_pool(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            _ => true,
        })
}

pub(super) fn invalid_rewind_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::CounterName)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::CounterName,
            CicsOperandName::CounterPool,
            CicsOperandName::CounterIncrement,
        ]))
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::CounterNoSuspend
            )
        })
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::CounterName => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_name(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterPool => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_pool(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterIncrement => match &operand.value {
                CicsOperandValue::Integer(value) => {
                    *value < 0
                        || plan.operation == CicsPlanOperation::RewindCounter
                            && *value > i64::from(i32::MAX)
                }
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            _ => true,
        })
}

pub(super) fn invalid_delete_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::CounterName)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::CounterName,
            CicsOperandName::CounterPool,
        ]))
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::CounterNoSuspend
            )
        })
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::CounterName => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_name(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::CounterPool => match &operand.value {
                CicsOperandValue::Literal(bytes) => invalid_pool(bytes),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            _ => true,
        })
}
