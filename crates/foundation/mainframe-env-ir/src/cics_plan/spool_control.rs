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

pub(super) fn invalid_open_input_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::SpoolUserId)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SpoolUserId,
            CicsOperandName::SpoolClass,
        ]))
        || !outputs.contains(&CicsOutputName::SpoolToken)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::SpoolUserId | CicsOperandName::SpoolClass => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || outputs.iter().any(|output| {
            !matches!(
                output,
                CicsOutputName::SpoolToken | CicsOutputName::Resp | CicsOutputName::Resp2
            )
        })
        || matches!(plan.condition, CicsCondition::Default)
}

pub(super) fn invalid_open_output_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::SpoolUserId)
        || !inputs.contains(&CicsOperandName::SpoolNode)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SpoolUserId,
            CicsOperandName::SpoolNode,
            CicsOperandName::SpoolClass,
            CicsOperandName::SpoolRecordLength,
            CicsOperandName::SpoolOutDescr,
        ]))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::SpoolUserId
            | CicsOperandName::SpoolNode
            | CicsOperandName::SpoolClass => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::SpoolRecordLength => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::SpoolOutDescr => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            _ => true,
        })
        || !outputs.contains(&CicsOutputName::SpoolToken)
        || outputs.iter().any(|output| {
            !matches!(
                output,
                CicsOutputName::SpoolToken | CicsOutputName::Resp | CicsOutputName::Resp2
            )
        })
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle
                    | CicsPlanOption::SpoolNoCc
                    | CicsPlanOption::SpoolAsa
                    | CicsPlanOption::SpoolMcc
                    | CicsPlanOption::SpoolPrint
                    | CicsPlanOption::SpoolPunch
            )
        })
        || [
            CicsPlanOption::SpoolNoCc,
            CicsPlanOption::SpoolAsa,
            CicsPlanOption::SpoolMcc,
        ]
        .iter()
        .filter(|option| plan.options.contains(option))
        .count()
            > 1
        || plan.options.contains(&CicsPlanOption::SpoolPrint)
            && plan.options.contains(&CicsPlanOption::SpoolPunch)
        || matches!(plan.condition, CicsCondition::Default)
}

pub(super) fn invalid_read_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs
        != &BTreeSet::from([
            CicsOperandName::SpoolToken,
            CicsOperandName::SpoolMaxFlength,
        ])
        || plan.operands.iter().any(|operand| {
            !matches!(
                (operand.name, &operand.value),
                (CicsOperandName::SpoolToken, CicsOperandValue::Storage(_))
                    | (
                        CicsOperandName::SpoolMaxFlength,
                        CicsOperandValue::Storage(_)
                    )
            )
        })
        || !outputs.contains(&CicsOutputName::Into)
        || outputs.iter().any(|output| {
            !matches!(
                output,
                CicsOutputName::Into
                    | CicsOutputName::SpoolToFlength
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || matches!(plan.condition, CicsCondition::Default)
}

pub(super) fn invalid_write_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::SpoolToken)
        || !inputs.contains(&CicsOperandName::SpoolFrom)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SpoolToken,
            CicsOperandName::SpoolFrom,
            CicsOperandName::SpoolFlength,
        ]))
        || plan.operands.iter().any(|operand| {
            !matches!(
                (operand.name, &operand.value),
                (CicsOperandName::SpoolToken, CicsOperandValue::Storage(_))
                    | (CicsOperandName::SpoolFrom, CicsOperandValue::Storage(_))
                    | (
                        CicsOperandName::SpoolFlength,
                        CicsOperandValue::Storage(_) | CicsOperandValue::LengthOf(_)
                    )
            )
        })
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::SpoolLine | CicsPlanOption::SpoolPage
            )
        })
        || plan.options.contains(&CicsPlanOption::SpoolLine)
            && plan.options.contains(&CicsPlanOption::SpoolPage)
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
        || matches!(plan.condition, CicsCondition::Default)
}
