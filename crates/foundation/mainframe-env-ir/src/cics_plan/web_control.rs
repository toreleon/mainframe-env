use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption,
    operand_value, output_target,
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

pub(super) fn invalid_read_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let names = [
        CicsOperandName::WebHttpHeaderName,
        CicsOperandName::WebQueryParmName,
        CicsOperandName::WebFormFieldName,
    ];
    let allowed = BTreeSet::from([
        names[0],
        names[1],
        names[2],
        CicsOperandName::WebSessionToken,
        CicsOperandName::WebNameLength,
        CicsOperandName::WebValueLength,
    ]);
    names.iter().filter(|name| inputs.contains(name)).count() != 1
        || !inputs.is_subset(&allowed)
        || !inputs.contains(&CicsOperandName::WebNameLength)
        || !inputs.contains(&CicsOperandName::WebValueLength)
        || inputs.contains(&CicsOperandName::WebSessionToken) && !inputs.contains(&names[0])
        || !outputs.contains(&CicsOutputName::WebValue)
        || !outputs.contains(&CicsOutputName::WebValueLength)
        || !matches!(
            operand_value(plan, CicsOperandName::WebValueLength),
            Some(CicsOperandValue::Storage(slot))
                if output_target(&plan.outputs, CicsOutputName::WebValueLength) == Some(slot)
        )
        || !matches!(
            operand_value(plan, CicsOperandName::WebNameLength),
            Some(
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            )
        )
        || names.iter().any(|name| {
            operand_value(plan, *name).is_some_and(|value| {
                !matches!(
                    value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                )
            })
        })
        || operand_value(plan, CicsOperandName::WebSessionToken).is_some_and(|value| {
            !matches!(
                value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
}

pub(super) fn invalid_start_browse_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let kinds = [
        CicsPlanOption::WebBrowseHttpHeader,
        CicsPlanOption::WebBrowseQueryParm,
        CicsPlanOption::WebBrowseFormField,
    ];
    let header = plan.options.contains(&kinds[0]);
    kinds
        .iter()
        .filter(|kind| plan.options.contains(kind))
        .count()
        != 1
        || !plan
            .options
            .iter()
            .all(|option| kinds.contains(option) || *option == CicsPlanOption::NoHandle)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::WebBrowseStartName,
            CicsOperandName::WebNameLength,
            CicsOperandName::WebSessionToken,
        ]))
        || inputs.contains(&CicsOperandName::WebBrowseStartName)
            != inputs.contains(&CicsOperandName::WebNameLength)
        || header && inputs.contains(&CicsOperandName::WebBrowseStartName)
        || !header && inputs.contains(&CicsOperandName::WebSessionToken)
        || operand_value(plan, CicsOperandName::WebBrowseStartName).is_some_and(|value| {
            !matches!(
                value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
        || operand_value(plan, CicsOperandName::WebNameLength).is_some_and(|value| {
            !matches!(
                value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            )
        })
        || operand_value(plan, CicsOperandName::WebSessionToken).is_some_and(|value| {
            !matches!(
                value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_read_next_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let kinds = [
        CicsPlanOption::WebBrowseHttpHeader,
        CicsPlanOption::WebBrowseQueryParm,
        CicsPlanOption::WebBrowseFormField,
    ];
    kinds
        .iter()
        .filter(|kind| plan.options.contains(kind))
        .count()
        != 1
        || !plan
            .options
            .iter()
            .all(|option| kinds.contains(option) || *option == CicsPlanOption::NoHandle)
        || !inputs.contains(&CicsOperandName::WebNameLength)
        || !inputs.contains(&CicsOperandName::WebValueLength)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::WebNameLength,
            CicsOperandName::WebValueLength,
            CicsOperandName::WebSessionToken,
        ]))
        || inputs.contains(&CicsOperandName::WebSessionToken)
            && !plan.options.contains(&CicsPlanOption::WebBrowseHttpHeader)
        || !outputs.contains(&CicsOutputName::WebBrowseName)
        || !outputs.contains(&CicsOutputName::WebBrowseNameLength)
        || !outputs.contains(&CicsOutputName::WebValue)
        || !outputs.contains(&CicsOutputName::WebValueLength)
        || !matches!(
            operand_value(plan, CicsOperandName::WebNameLength),
            Some(CicsOperandValue::Storage(slot))
                if output_target(&plan.outputs, CicsOutputName::WebBrowseNameLength) == Some(slot)
        )
        || !matches!(
            operand_value(plan, CicsOperandName::WebValueLength),
            Some(CicsOperandValue::Storage(slot))
                if output_target(&plan.outputs, CicsOutputName::WebValueLength) == Some(slot)
        )
        || operand_value(plan, CicsOperandName::WebSessionToken).is_some_and(|value| {
            !matches!(
                value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
}

pub(super) fn invalid_end_browse_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let kinds = [
        CicsPlanOption::WebBrowseHttpHeader,
        CicsPlanOption::WebBrowseQueryParm,
        CicsPlanOption::WebBrowseFormField,
    ];
    kinds
        .iter()
        .filter(|kind| plan.options.contains(kind))
        .count()
        != 1
        || !plan
            .options
            .iter()
            .all(|option| kinds.contains(option) || *option == CicsPlanOption::NoHandle)
        || !inputs.is_subset(&BTreeSet::from([CicsOperandName::WebSessionToken]))
        || inputs.contains(&CicsOperandName::WebSessionToken)
            && !plan.options.contains(&CicsPlanOption::WebBrowseHttpHeader)
        || operand_value(plan, CicsOperandName::WebSessionToken).is_some_and(|value| {
            !matches!(
                value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_write_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let required = [
        CicsOperandName::WebHttpHeaderName,
        CicsOperandName::WebNameLength,
        CicsOperandName::WebHeaderValue,
        CicsOperandName::WebValueLength,
    ];
    let allowed = BTreeSet::from([
        required[0],
        required[1],
        required[2],
        required[3],
        CicsOperandName::WebSessionToken,
    ]);
    !required.iter().all(|name| inputs.contains(name))
        || !inputs.is_subset(&allowed)
        || !plan
            .options
            .iter()
            .all(|option| *option == CicsPlanOption::NoHandle)
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
        || [required[0], required[2], CicsOperandName::WebSessionToken]
            .into_iter()
            .any(|name| {
                operand_value(plan, name).is_some_and(|value| {
                    !matches!(
                        value,
                        CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                    )
                })
            })
        || [required[1], required[3]].into_iter().any(|name| {
            !matches!(
                operand_value(plan, name),
                Some(
                    CicsOperandValue::Integer(_)
                        | CicsOperandValue::Storage(_)
                        | CicsOperandValue::LengthOf(_)
                )
            )
        })
}

pub(super) fn invalid_send_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let client = inputs.contains(&CicsOperandName::WebSessionToken);
    let body = inputs.contains(&CicsOperandName::WebFrom);
    let document = inputs.contains(&CicsOperandName::WebDocumentToken);
    let allowed = BTreeSet::from([
        CicsOperandName::WebSessionToken,
        CicsOperandName::WebMethod,
        CicsOperandName::WebAction,
        CicsOperandName::WebCloseStatus,
        CicsOperandName::WebDocumentToken,
        CicsOperandName::WebStatusCode,
        CicsOperandName::WebStatusText,
        CicsOperandName::WebStatusLength,
        CicsOperandName::WebFrom,
        CicsOperandName::WebFromLength,
        CicsOperandName::WebPathInput,
        CicsOperandName::WebPathLength,
        CicsOperandName::WebQueryInput,
        CicsOperandName::WebQueryStringLength,
        CicsOperandName::WebMediaType,
        CicsOperandName::WebSendUriMap,
    ]);
    !inputs.is_subset(&allowed)
        || !plan
            .options
            .iter()
            .all(|option| *option == CicsPlanOption::NoHandle)
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
        || body && document
        || body != inputs.contains(&CicsOperandName::WebFromLength)
        || inputs.contains(&CicsOperandName::WebPathInput)
            != inputs.contains(&CicsOperandName::WebPathLength)
        || inputs.contains(&CicsOperandName::WebQueryInput)
            != inputs.contains(&CicsOperandName::WebQueryStringLength)
        || inputs.contains(&CicsOperandName::WebStatusText)
            != inputs.contains(&CicsOperandName::WebStatusLength)
        || client != inputs.contains(&CicsOperandName::WebMethod)
        || client
            && [
                CicsOperandName::WebStatusCode,
                CicsOperandName::WebStatusText,
                CicsOperandName::WebStatusLength,
                CicsOperandName::WebAction,
            ]
            .into_iter()
            .any(|name| inputs.contains(&name))
        || !client
            && [
                CicsOperandName::WebPathInput,
                CicsOperandName::WebPathLength,
                CicsOperandName::WebQueryInput,
                CicsOperandName::WebQueryStringLength,
                CicsOperandName::WebSendUriMap,
            ]
            .into_iter()
            .any(|name| inputs.contains(&name))
        || !client && !body && !document
        || [
            CicsOperandName::WebMethod,
            CicsOperandName::WebAction,
            CicsOperandName::WebCloseStatus,
        ]
        .into_iter()
        .any(|name| {
            operand_value(plan, name)
                .is_some_and(|value| !matches!(value, CicsOperandValue::Literal(_)))
        })
        || [
            CicsOperandName::WebSessionToken,
            CicsOperandName::WebDocumentToken,
            CicsOperandName::WebStatusText,
            CicsOperandName::WebFrom,
            CicsOperandName::WebPathInput,
            CicsOperandName::WebQueryInput,
            CicsOperandName::WebMediaType,
            CicsOperandName::WebSendUriMap,
        ]
        .into_iter()
        .any(|name| {
            operand_value(plan, name).is_some_and(|value| {
                !matches!(
                    value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                )
            })
        })
        || [
            CicsOperandName::WebStatusCode,
            CicsOperandName::WebStatusLength,
            CicsOperandName::WebFromLength,
            CicsOperandName::WebPathLength,
            CicsOperandName::WebQueryStringLength,
        ]
        .into_iter()
        .any(|name| {
            operand_value(plan, name).is_some_and(|value| {
                !matches!(
                    value,
                    CicsOperandValue::Integer(_)
                        | CicsOperandValue::Storage(_)
                        | CicsOperandValue::LengthOf(_)
                )
            })
        })
}

pub(super) fn invalid_retrieve_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.is_empty()
        || !plan
            .options
            .iter()
            .all(|option| *option == CicsPlanOption::NoHandle)
        || !outputs.contains(&CicsOutputName::WebRetrieveDocumentToken)
        || outputs.iter().any(|name| {
            !matches!(
                name,
                CicsOutputName::WebRetrieveDocumentToken
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
            )
        })
}
