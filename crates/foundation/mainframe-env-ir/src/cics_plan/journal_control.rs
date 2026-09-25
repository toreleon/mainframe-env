use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) fn invalid_wait_journal_name_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs =
        BTreeSet::from([CicsOperandName::JournalName, CicsOperandName::JournalReqId]);
    !inputs.contains(&CicsOperandName::JournalName)
        || !inputs.is_subset(&allowed_inputs)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::JournalName => match &operand.value {
                CicsOperandValue::Literal(value) => {
                    !(1..=8).contains(&value.len())
                        || !value.iter().all(|byte| {
                            byte.is_ascii_uppercase()
                                || byte.is_ascii_digit()
                                || matches!(byte, b'$' | b'@' | b'#')
                        })
                }
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::JournalReqId => !matches!(operand.value, CicsOperandValue::Storage(_)),
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

pub(super) fn invalid_wait_journal_num_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs =
        BTreeSet::from([CicsOperandName::JournalNum, CicsOperandName::JournalReqId]);
    !inputs.contains(&CicsOperandName::JournalNum)
        || !inputs.is_subset(&allowed_inputs)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::JournalNum => match &operand.value {
                CicsOperandValue::Integer(value) => !(1..=99).contains(value),
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::JournalReqId => !matches!(operand.value, CicsOperandValue::Storage(_)),
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

pub(super) fn invalid_write_journal_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let selector = match plan.operation {
        CicsPlanOperation::WriteJournalName => CicsOperandName::JournalName,
        CicsPlanOperation::WriteJournalNum => CicsOperandName::JournalNum,
        _ => unreachable!("only journal writes delegate write shape"),
    };
    let allowed_inputs = BTreeSet::from([
        selector,
        CicsOperandName::JournalTypeId,
        CicsOperandName::JournalFrom,
        CicsOperandName::JournalFlength,
        CicsOperandName::JournalPrefix,
        CicsOperandName::JournalPfxLeng,
    ]);
    ![
        selector,
        CicsOperandName::JournalTypeId,
        CicsOperandName::JournalFrom,
    ]
    .iter()
    .all(|name| inputs.contains(name))
        || !inputs.is_subset(&allowed_inputs)
        || inputs.contains(&CicsOperandName::JournalPfxLeng)
            && !inputs.contains(&CicsOperandName::JournalPrefix)
        || plan.options.contains(&CicsPlanOption::Wait)
            && outputs.contains(&CicsOutputName::JournalReqId)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::JournalName => {
                selector != CicsOperandName::JournalName
                    || match &operand.value {
                        CicsOperandValue::Literal(value) => {
                            !(1..=8).contains(&value.len())
                                || !value.iter().all(|byte| {
                                    byte.is_ascii_uppercase()
                                        || byte.is_ascii_digit()
                                        || matches!(byte, b'$' | b'@' | b'#')
                                })
                        }
                        CicsOperandValue::Storage(_) => false,
                        _ => true,
                    }
            }
            CicsOperandName::JournalNum => {
                selector != CicsOperandName::JournalNum
                    || match &operand.value {
                        CicsOperandValue::Integer(value) => !(1..=99).contains(value),
                        CicsOperandValue::Storage(_) => false,
                        _ => true,
                    }
            }
            CicsOperandName::JournalTypeId => match &operand.value {
                CicsOperandValue::Literal(value) => value.len() != 2,
                CicsOperandValue::Storage(_) => false,
                _ => true,
            },
            CicsOperandName::JournalFrom | CicsOperandName::JournalPrefix => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::JournalFlength => !matches!(
                operand.value,
                CicsOperandValue::Integer(0..) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::JournalPfxLeng => !matches!(
                operand.value,
                CicsOperandValue::Integer(0..=65_535) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::JournalReqId,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::Wait | CicsPlanOption::NoSuspend
            )
        })
}
