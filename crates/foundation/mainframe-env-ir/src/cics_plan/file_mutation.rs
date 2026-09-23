use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let resources = usize::from(inputs.contains(&CicsOperandName::File))
        + usize::from(inputs.contains(&CicsOperandName::Dataset));
    let explicit_key_write = plan.operation == CicsPlanOperation::Write;
    let writes_record = matches!(
        plan.operation,
        CicsPlanOperation::Write | CicsPlanOperation::Rewrite
    );
    let rewrite = plan.operation == CicsPlanOperation::Rewrite;
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::File,
        CicsOperandName::Dataset,
        CicsOperandName::From,
        CicsOperandName::Ridfld,
        CicsOperandName::Length,
        CicsOperandName::KeyLength,
    ]);
    resources != 1
        || !inputs.is_subset(&allowed_inputs)
        || (explicit_key_write && !inputs.contains(&CicsOperandName::Ridfld))
        || (inputs.contains(&CicsOperandName::KeyLength)
            && !inputs.contains(&CicsOperandName::Ridfld))
        || writes_record != inputs.contains(&CicsOperandName::From)
        || (rewrite
            && (inputs.contains(&CicsOperandName::Ridfld)
                || inputs.contains(&CicsOperandName::KeyLength)))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::From | CicsOperandName::Ridfld => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::Length => {
                !writes_record
                    || !matches!(
                        operand.value,
                        CicsOperandValue::Integer(0..=32_767)
                            | CicsOperandValue::Storage(_)
                            | CicsOperandValue::LengthOf(_)
                    )
            }
            CicsOperandName::KeyLength => !matches!(
                operand.value,
                CicsOperandValue::Integer(1..=32_767)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => false,
        })
        || match (
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::From),
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Length),
        ) {
            (
                Some(super::CicsNamedOperand {
                    value: CicsOperandValue::Storage(from),
                    ..
                }),
                Some(super::CicsNamedOperand {
                    value: CicsOperandValue::LengthOf(length),
                    ..
                }),
            ) => from != length,
            _ => false,
        }
        || match (
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Ridfld),
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::KeyLength),
        ) {
            (
                Some(super::CicsNamedOperand {
                    value: CicsOperandValue::Storage(ridfld),
                    ..
                }),
                Some(super::CicsNamedOperand {
                    value: CicsOperandValue::LengthOf(length),
                    ..
                }),
            ) => ridfld != length,
            _ => false,
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
