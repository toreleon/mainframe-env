use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_query_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::ResId)
        || inputs.contains(&CicsOperandName::ResClass) == inputs.contains(&CicsOperandName::ResType)
        || inputs.contains(&CicsOperandName::ResIdLength)
            != inputs.contains(&CicsOperandName::ResClass)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::ResId,
            CicsOperandName::ResClass,
            CicsOperandName::ResType,
            CicsOperandName::ResIdLength,
            CicsOperandName::LogMessage,
            CicsOperandName::SecurityUserId,
        ]))
        || !outputs.iter().any(|name| {
            matches!(
                name,
                CicsOutputName::SecurityRead
                    | CicsOutputName::SecurityUpdate
                    | CicsOutputName::SecurityControl
                    | CicsOutputName::SecurityAlter
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::ResIdLength | CicsOperandName::LogMessage => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}

pub(super) fn invalid_verify_password_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::SecurityUserId)
        || !inputs.contains(&CicsOperandName::SecurityPassword)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SecurityUserId,
            CicsOperandName::SecurityGroupId,
            CicsOperandName::SecurityPassword,
        ]))
        || outputs.iter().any(|name| {
            !matches!(
                name,
                CicsOutputName::SecurityChangeTime
                    | CicsOutputName::SecurityDaysLeft
                    | CicsOutputName::SecurityEsmReason
                    | CicsOutputName::SecurityEsmResp
                    | CicsOutputName::SecurityExpiryTime
                    | CicsOutputName::SecurityInvalidCount
                    | CicsOutputName::SecurityLastUseTime
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::SecurityPassword => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}

pub(super) fn invalid_verify_phrase_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::SecurityUserId)
        || !inputs.contains(&CicsOperandName::SecurityPhrase)
        || !inputs.contains(&CicsOperandName::SecurityPhraseLen)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SecurityUserId,
            CicsOperandName::SecurityGroupId,
            CicsOperandName::SecurityPhrase,
            CicsOperandName::SecurityPhraseLen,
        ]))
        || outputs.iter().any(|name| {
            !matches!(
                name,
                CicsOutputName::SecurityChangeTime
                    | CicsOutputName::SecurityDaysLeft
                    | CicsOutputName::SecurityEsmReason
                    | CicsOutputName::SecurityEsmResp
                    | CicsOutputName::SecurityExpiryTime
                    | CicsOutputName::SecurityInvalidCount
                    | CicsOutputName::SecurityLastUseTime
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::SecurityPhrase => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::SecurityPhraseLen => !matches!(
                operand.value,
                CicsOperandValue::Integer(1..=100) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}

pub(super) fn invalid_change_password_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::SecurityUserId)
        || !inputs.contains(&CicsOperandName::SecurityPassword)
        || !inputs.contains(&CicsOperandName::SecurityNewPassword)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SecurityUserId,
            CicsOperandName::SecurityPassword,
            CicsOperandName::SecurityNewPassword,
        ]))
        || outputs.iter().any(|name| {
            !matches!(
                name,
                CicsOutputName::SecurityChangeTime
                    | CicsOutputName::SecurityDaysLeft
                    | CicsOutputName::SecurityEsmReason
                    | CicsOutputName::SecurityEsmResp
                    | CicsOutputName::SecurityExpiryTime
                    | CicsOutputName::SecurityInvalidCount
                    | CicsOutputName::SecurityLastUseTime
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::SecurityPassword | CicsOperandName::SecurityNewPassword => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}
