use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
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
