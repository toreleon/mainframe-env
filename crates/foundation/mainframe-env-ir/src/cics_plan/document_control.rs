use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption,
    operand_value, output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_create_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::From,
        CicsOperandName::Text,
        CicsOperandName::Binary,
        CicsOperandName::FromDocument,
        CicsOperandName::Template,
        CicsOperandName::Length,
        CicsOperandName::SymbolList,
        CicsOperandName::ListLength,
        CicsOperandName::Delimiter,
        CicsOperandName::HostCodePage,
    ]);
    let content_sources = [
        CicsOperandName::From,
        CicsOperandName::Text,
        CicsOperandName::Binary,
        CicsOperandName::FromDocument,
        CicsOperandName::Template,
    ]
    .into_iter()
    .filter(|name| inputs.contains(name))
    .count();
    let buffered_source = [
        CicsOperandName::From,
        CicsOperandName::Text,
        CicsOperandName::Binary,
    ]
    .into_iter()
    .any(|name| inputs.contains(&name));
    let has_symbols = inputs.contains(&CicsOperandName::SymbolList);
    !inputs.is_subset(&allowed)
        || content_sources > 1
        || inputs.contains(&CicsOperandName::Length) != buffered_source
        || inputs.contains(&CicsOperandName::ListLength) != has_symbols
        || (inputs.contains(&CicsOperandName::Delimiter) && !has_symbols)
        || (plan.options.contains(&CicsPlanOption::Unescaped) && !has_symbols)
        || (inputs.contains(&CicsOperandName::HostCodePage)
            && !inputs
                .intersection(&BTreeSet::from([
                    CicsOperandName::From,
                    CicsOperandName::Text,
                    CicsOperandName::Template,
                ]))
                .next()
                .is_some())
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::From
            | CicsOperandName::Text
            | CicsOperandName::Binary
            | CicsOperandName::FromDocument
            | CicsOperandName::Template
            | CicsOperandName::SymbolList
            | CicsOperandName::Delimiter
            | CicsOperandName::HostCodePage => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::Length | CicsOperandName::ListLength => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => true,
        })
        || !outputs.contains(&CicsOutputName::DocumentToken)
        || output_target(&plan.outputs, CicsOutputName::DocumentToken).is_none()
        || outputs.contains(&CicsOutputName::Into)
        || operand_value(plan, CicsOperandName::Length)
            .is_some_and(|value| matches!(value, CicsOperandValue::Integer(value) if *value < 0))
        || operand_value(plan, CicsOperandName::ListLength)
            .is_some_and(|value| matches!(value, CicsOperandValue::Integer(value) if *value < 1))
}

pub(super) fn invalid_delete_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs != &BTreeSet::from([CicsOperandName::DocumentToken])
        || plan.operands.iter().any(|operand| {
            operand.name != CicsOperandName::DocumentToken
                || !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_insert_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::DocumentToken,
        CicsOperandName::From,
        CicsOperandName::Text,
        CicsOperandName::Binary,
        CicsOperandName::FromDocument,
        CicsOperandName::Template,
        CicsOperandName::Symbol,
        CicsOperandName::Length,
        CicsOperandName::Bookmark,
        CicsOperandName::AtBookmark,
        CicsOperandName::ToBookmark,
        CicsOperandName::HostCodePage,
    ]);
    let sources = [
        CicsOperandName::From,
        CicsOperandName::Text,
        CicsOperandName::Binary,
        CicsOperandName::FromDocument,
        CicsOperandName::Template,
        CicsOperandName::Symbol,
    ]
    .into_iter()
    .filter(|name| inputs.contains(name))
    .count();
    let buffered = [
        CicsOperandName::From,
        CicsOperandName::Text,
        CicsOperandName::Binary,
    ]
    .into_iter()
    .any(|name| inputs.contains(&name));
    !inputs.is_subset(&allowed)
        || !inputs.contains(&CicsOperandName::DocumentToken)
        || sources > 1
        || (sources == 0 && !inputs.contains(&CicsOperandName::Bookmark))
        || inputs.contains(&CicsOperandName::Length) != buffered
        || (inputs.contains(&CicsOperandName::HostCodePage)
            && ![
                CicsOperandName::Text,
                CicsOperandName::Symbol,
                CicsOperandName::Template,
            ]
            .into_iter()
            .any(|name| inputs.contains(&name)))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::DocumentToken | CicsOperandName::FromDocument => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::From | CicsOperandName::Text | CicsOperandName::Binary => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::Template
            | CicsOperandName::Symbol
            | CicsOperandName::Bookmark
            | CicsOperandName::AtBookmark
            | CicsOperandName::ToBookmark
            | CicsOperandName::HostCodePage => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::Length => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => true,
        })
        || operand_value(plan, CicsOperandName::Length)
            .is_some_and(|value| matches!(value, CicsOperandValue::Integer(value) if *value < 0))
        || outputs.iter().any(|output| {
            !matches!(
                output,
                CicsOutputName::DocumentSize | CicsOutputName::Resp | CicsOutputName::Resp2
            )
        })
}
