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

pub(super) fn invalid_change_phrase_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    ![
        CicsOperandName::SecurityUserId,
        CicsOperandName::SecurityPhrase,
        CicsOperandName::SecurityPhraseLen,
        CicsOperandName::SecurityNewPhrase,
        CicsOperandName::SecurityNewPhraseLen,
    ]
    .iter()
    .all(|name| inputs.contains(name))
        || inputs.len() != 5
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
            CicsOperandName::SecurityPhrase | CicsOperandName::SecurityNewPhrase => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::SecurityPhraseLen | CicsOperandName::SecurityNewPhraseLen => {
                !matches!(
                    operand.value,
                    CicsOperandValue::Integer(1..=100) | CicsOperandValue::Storage(_)
                )
            }
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}

pub(super) fn invalid_request_passticket_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs != &BTreeSet::from([CicsOperandName::SecurityEsmAppName])
        || !outputs.contains(&CicsOutputName::SecurityPassTicket)
        || outputs.iter().any(|name| {
            !matches!(
                name,
                CicsOutputName::SecurityPassTicket
                    | CicsOutputName::SecurityEsmResp
                    | CicsOutputName::SecurityEsmReason
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan
            .operands
            .iter()
            .any(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
}

pub(super) fn invalid_signon_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let password = inputs.contains(&CicsOperandName::SecurityPassword);
    let phrase = inputs.contains(&CicsOperandName::SecurityPhrase);
    !inputs.contains(&CicsOperandName::SecurityUserId)
        || password == phrase
        || phrase != inputs.contains(&CicsOperandName::SecurityPhraseLen)
        || inputs.contains(&CicsOperandName::SecurityNewPassword) && !password
        || inputs.contains(&CicsOperandName::SecurityNewPhrase) && !phrase
        || inputs.contains(&CicsOperandName::SecurityNewPhrase)
            != inputs.contains(&CicsOperandName::SecurityNewPhraseLen)
        || inputs.contains(&CicsOperandName::SecurityLanguageCode)
            && inputs.contains(&CicsOperandName::SecurityNatLang)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::SecurityUserId,
            CicsOperandName::SecurityGroupId,
            CicsOperandName::SecurityPassword,
            CicsOperandName::SecurityNewPassword,
            CicsOperandName::SecurityPhrase,
            CicsOperandName::SecurityPhraseLen,
            CicsOperandName::SecurityNewPhrase,
            CicsOperandName::SecurityNewPhraseLen,
            CicsOperandName::SecurityLanguageCode,
            CicsOperandName::SecurityNatLang,
            CicsOperandName::SecurityOidCard,
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
                    | CicsOutputName::SecurityLangInUse
                    | CicsOutputName::SecurityNatLangInUse
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::SecurityPassword
            | CicsOperandName::SecurityNewPassword
            | CicsOperandName::SecurityPhrase
            | CicsOperandName::SecurityNewPhrase
            | CicsOperandName::SecurityOidCard => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::SecurityPhraseLen => !matches!(
                operand.value,
                CicsOperandValue::Integer(1..=100) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::SecurityNewPhraseLen => !matches!(
                operand.value,
                CicsOperandValue::Integer(0..=100) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}

pub(super) fn invalid_signoff_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.is_empty()
        || outputs
            .iter()
            .any(|name| !matches!(name, CicsOutputName::Resp | CicsOutputName::Resp2))
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
}
