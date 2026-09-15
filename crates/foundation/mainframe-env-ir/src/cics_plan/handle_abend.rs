use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_abend_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.is_subset(&BTreeSet::from([CicsOperandName::Abcode]))
        || plan.operands.iter().any(|operand| {
            operand.name != CicsOperandName::Abcode
                || !matches!(
                    &operand.value,
                    CicsOperandValue::Literal(bytes) if matches!(bytes.len(), 1..=4)
                ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::Cancel | CicsPlanOption::NoDump | CicsPlanOption::NoHandle
            )
        })
        || outputs.contains(&CicsOutputName::Into)
}

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let actions = inputs.len()
        + usize::from(plan.options.contains(&CicsPlanOption::Cancel))
        + usize::from(plan.options.contains(&CicsPlanOption::Reset));
    !inputs.is_subset(&BTreeSet::from([
        CicsOperandName::Label,
        CicsOperandName::Program,
    ])) || actions > 1 || plan.operands.iter().any(|operand| match operand.name {
        CicsOperandName::Label => {
            !matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_control_name(bytes))
        }
        CicsOperandName::Program => {
            !matches!(
                &operand.value,
                CicsOperandValue::Literal(bytes) if valid_program_name(bytes)
            ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        }
        _ => true,
    }) || plan.options.iter().any(|option| {
        !matches!(
            option,
            CicsPlanOption::Cancel | CicsPlanOption::Reset | CicsPlanOption::NoHandle
        )
    }) || outputs.contains(&CicsOutputName::Into)
}

fn valid_control_name(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

fn valid_program_name(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 1..=8) && bytes.iter().all(u8::is_ascii_alphanumeric)
}
