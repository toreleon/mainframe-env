use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, operand_value, output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_parse_url_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let required = [CicsOperandName::WebUrl, CicsOperandName::WebUrlLength];
    let allowed = BTreeSet::from([
        CicsOperandName::WebUrl,
        CicsOperandName::WebUrlLength,
        CicsOperandName::WebHostLength,
        CicsOperandName::WebPathLength,
        CicsOperandName::WebQueryStringLength,
    ]);
    !required.iter().all(|name| inputs.contains(name))
        || !inputs.is_subset(&allowed)
        || !matches!(
            operand_value(plan, CicsOperandName::WebUrl),
            Some(CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_))
        )
        || !matches!(
            operand_value(plan, CicsOperandName::WebUrlLength),
            Some(
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            )
        )
        || [
            (
                CicsOperandName::WebHostLength,
                CicsOutputName::WebHostLength,
                CicsOutputName::WebHost,
            ),
            (
                CicsOperandName::WebPathLength,
                CicsOutputName::WebPathLength,
                CicsOutputName::WebPath,
            ),
            (
                CicsOperandName::WebQueryStringLength,
                CicsOutputName::WebQueryStringLength,
                CicsOutputName::WebQueryString,
            ),
        ]
        .into_iter()
        .any(|(input, output, buffer)| {
            let value = operand_value(plan, input);
            let target = output_target(&plan.outputs, output);
            value.is_some() != target.is_some()
                || value.is_some() != outputs.contains(&buffer)
                || !matches!(value, None | Some(CicsOperandValue::Storage(_)))
                || matches!(value, Some(CicsOperandValue::Storage(slot)) if target != Some(slot))
        })
        || !outputs
            .iter()
            .any(|name| !matches!(name, CicsOutputName::Resp | CicsOutputName::Resp2))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::WebUrl | CicsOperandName::WebUrlLength => false,
            CicsOperandName::WebHostLength
            | CicsOperandName::WebPathLength
            | CicsOperandName::WebQueryStringLength => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            _ => true,
        })
}
