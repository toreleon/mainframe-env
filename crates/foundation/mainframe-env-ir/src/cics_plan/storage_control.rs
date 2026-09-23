use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_getmain_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let flength_form = inputs.contains(&CicsOperandName::Flength);
    let length_form = inputs.contains(&CicsOperandName::Length);
    flength_form == length_form
        || inputs.iter().any(|name| {
            !matches!(
                name,
                CicsOperandName::Flength | CicsOperandName::Length | CicsOperandName::InitImage
            )
        })
        || !outputs.contains(&CicsOutputName::SetPointer)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::SetPointer,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Flength | CicsOperandName::Length => !matches!(
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

const AMODE64_ABI: &[u8] = b"mainframe-env.cics-amode64-nonle@1";

pub(super) fn invalid_getmain64_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::Flength64)
        || !inputs.contains(&CicsOperandName::Abi64)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::Flength64,
            CicsOperandName::Location64,
            CicsOperandName::Abi64,
        ]))
        || !outputs.contains(&CicsOutputName::SetPointer64)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::SetPointer64,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Flength64 => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::Location64 => !matches!(
                &operand.value,
                CicsOperandValue::Literal(value) if matches!(value.as_slice(), b"LOC24" | b"LOC31")
            ),
            CicsOperandName::Abi64 => !matches!(
                &operand.value,
                CicsOperandValue::Literal(value) if value == AMODE64_ABI
            ),
            _ => true,
        })
        || (plan.options.contains(&CicsPlanOption::CicsDataKey64)
            && plan.options.contains(&CicsPlanOption::UserDataKey64))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle
                    | CicsPlanOption::NoSuspend
                    | CicsPlanOption::CicsDataKey64
                    | CicsPlanOption::UserDataKey64
                    | CicsPlanOption::Shared64
                    | CicsPlanOption::Executable64
            )
        })
}
