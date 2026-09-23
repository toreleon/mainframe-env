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

pub(super) fn invalid_open_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let urimap = inputs.contains(&CicsOperandName::WebUriMap);
    let direct = inputs.contains(&CicsOperandName::WebHost)
        && inputs.contains(&CicsOperandName::WebHostLength)
        && inputs.contains(&CicsOperandName::WebScheme);
    let allowed = BTreeSet::from([
        CicsOperandName::WebUriMap,
        CicsOperandName::WebHost,
        CicsOperandName::WebHostLength,
        CicsOperandName::WebPortNumber,
        CicsOperandName::WebScheme,
        CicsOperandName::WebCertificate,
        CicsOperandName::WebCodePage,
    ]);
    !inputs.is_subset(&allowed)
        || urimap == direct
        || urimap && inputs.len() != 1 && inputs != &BTreeSet::from([
            CicsOperandName::WebUriMap,
            CicsOperandName::WebCodePage,
        ])
        || !outputs.contains(&CicsOutputName::WebSessionToken)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::WebHost
            | CicsOperandName::WebScheme
            | CicsOperandName::WebUriMap
            | CicsOperandName::WebCertificate
            | CicsOperandName::WebCodePage => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::WebHostLength | CicsOperandName::WebPortNumber => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => true,
        })
        || operand_value(plan, CicsOperandName::WebHostLength)
            .is_some_and(|value| matches!(value, CicsOperandValue::Integer(number) if *number < 1))
        || operand_value(plan, CicsOperandName::WebPortNumber)
            .is_some_and(|value| matches!(value, CicsOperandValue::Integer(number) if !(1..=65535).contains(number)))
}

pub(super) fn invalid_close_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs != &BTreeSet::from([CicsOperandName::WebSessionToken])
        || !matches!(
            operand_value(plan, CicsOperandName::WebSessionToken),
            Some(CicsOperandValue::Storage(_) | CicsOperandValue::Literal(_))
        )
        || outputs
            .iter()
            .any(|name| !matches!(name, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_extract_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed = BTreeSet::from([
        CicsOperandName::WebSessionToken,
        CicsOperandName::WebHostLength,
        CicsOperandName::WebPathLength,
        CicsOperandName::WebQueryStringLength,
        CicsOperandName::WebMethodLength,
        CicsOperandName::WebVersionLength,
        CicsOperandName::WebRealmLength,
    ]);
    !inputs.is_subset(&allowed)
        || !outputs
            .iter()
            .any(|name| !matches!(name, CicsOutputName::Resp | CicsOutputName::Resp2))
        || inputs.contains(&CicsOperandName::WebSessionToken)
            && !matches!(
                operand_value(plan, CicsOperandName::WebSessionToken),
                Some(CicsOperandValue::Storage(_) | CicsOperandValue::Literal(_))
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
            (
                CicsOperandName::WebMethodLength,
                CicsOutputName::WebMethodLength,
                CicsOutputName::WebHttpMethod,
            ),
            (
                CicsOperandName::WebVersionLength,
                CicsOutputName::WebVersionLength,
                CicsOutputName::WebHttpVersion,
            ),
            (
                CicsOperandName::WebRealmLength,
                CicsOutputName::WebRealmLength,
                CicsOutputName::WebRealm,
            ),
        ]
        .into_iter()
        .any(|(input, length, buffer)| {
            let value = operand_value(plan, input);
            let target = output_target(&plan.outputs, length);
            value.is_some() != target.is_some()
                || value.is_some() != outputs.contains(&buffer)
                || !matches!(value, None | Some(CicsOperandValue::Storage(_)))
                || matches!(value, Some(CicsOperandValue::Storage(slot)) if target != Some(slot))
        })
}
