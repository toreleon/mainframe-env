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
    if plan
        .options
        .iter()
        .any(|option| *option != CicsPlanOption::NoHandle)
    {
        return true;
    }
    match plan.operation {
        CicsPlanOperation::TransformDataToJson | CicsPlanOperation::TransformJsonToData => {
            invalid_data_to_json(plan, inputs)
        }
        CicsPlanOperation::TransformDataToXml => invalid_data_to_xml(plan, inputs, outputs),
        _ => true,
    }
}

fn invalid_data_to_json(plan: &CicsEffectPlan, inputs: &BTreeSet<CicsOperandName>) -> bool {
    let required = BTreeSet::from([
        CicsOperandName::Channel,
        CicsOperandName::InContainer,
        CicsOperandName::Transformer,
    ]);
    let allowed = BTreeSet::from([
        CicsOperandName::Channel,
        CicsOperandName::InContainer,
        CicsOperandName::OutContainer,
        CicsOperandName::Transformer,
    ]);
    !required.is_subset(inputs)
        || !inputs.is_subset(&allowed)
        || plan.operands.iter().any(invalid_text_operand)
}

fn invalid_data_to_xml(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let required = BTreeSet::from([
        CicsOperandName::Channel,
        CicsOperandName::DataContainer,
        CicsOperandName::XmlContainer,
        CicsOperandName::XmlTransform,
    ]);
    let allowed = BTreeSet::from([
        CicsOperandName::Channel,
        CicsOperandName::DataContainer,
        CicsOperandName::XmlContainer,
        CicsOperandName::XmlTransform,
        CicsOperandName::ElementNameLength,
        CicsOperandName::ElementNamespaceLength,
        CicsOperandName::TypeNameLength,
        CicsOperandName::TypeNamespaceLength,
    ]);
    !required.is_subset(inputs)
        || !inputs.is_subset(&allowed)
        || plan.operands.iter().any(|operand| {
            if matches!(
                operand.name,
                CicsOperandName::ElementNameLength
                    | CicsOperandName::ElementNamespaceLength
                    | CicsOperandName::TypeNameLength
                    | CicsOperandName::TypeNamespaceLength
            ) {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            } else {
                invalid_text_operand(operand)
            }
        })
        || metadata_pair_differs(
            inputs,
            outputs,
            CicsOperandName::ElementNameLength,
            CicsOutputName::ElementName,
            CicsOutputName::ElementNameLength,
        )
        || metadata_pair_differs(
            inputs,
            outputs,
            CicsOperandName::ElementNamespaceLength,
            CicsOutputName::ElementNamespace,
            CicsOutputName::ElementNamespaceLength,
        )
        || metadata_pair_differs(
            inputs,
            outputs,
            CicsOperandName::TypeNameLength,
            CicsOutputName::TypeName,
            CicsOutputName::TypeNameLength,
        )
        || metadata_pair_differs(
            inputs,
            outputs,
            CicsOperandName::TypeNamespaceLength,
            CicsOutputName::TypeNamespace,
            CicsOutputName::TypeNamespaceLength,
        )
}

fn invalid_text_operand(operand: &super::CicsNamedOperand) -> bool {
    !matches!(
        operand.value,
        CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
    )
}

fn metadata_pair_differs(
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    input_length: CicsOperandName,
    output_text: CicsOutputName,
    output_length: CicsOutputName,
) -> bool {
    let input = inputs.contains(&input_length);
    input != outputs.contains(&output_text) || input != outputs.contains(&output_length)
}
