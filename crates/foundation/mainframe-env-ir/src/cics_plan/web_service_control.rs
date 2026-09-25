use super::{CicsEffectPlan, CicsOperandName as N, CicsOperandValue, CicsOutputName as O};
use super::{CicsPlanOperation as P, CicsPlanOption, operand_value, output_target};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<N>,
    outputs: &BTreeSet<O>,
) -> bool {
    let (allowed, required, results): (&[N], &[N], &[O]) = match plan.operation {
        P::InvokeService => (
            &[
                N::Service,
                N::ServiceOperation,
                N::Channel,
                N::Uri,
                N::UriMap,
                N::Scope,
                N::ScopeLen,
            ],
            &[N::Service],
            &[],
        ),
        P::SoapFaultCreate => (
            &[
                N::FaultCode,
                N::FaultCodeStr,
                N::FaultCodeLen,
                N::FaultString,
                N::FaultStrLen,
                N::NatLang,
                N::SoapRole,
                N::RoleLength,
                N::FaultActor,
                N::FaultActLen,
                N::Detail,
                N::DetailLength,
                N::FromCcsid,
            ],
            &[N::FaultString, N::FaultStrLen],
            &[],
        ),
        P::SoapFaultAdd => (
            &[
                N::FaultString,
                N::FaultStrLen,
                N::NatLang,
                N::SubcodeStr,
                N::SubcodeLen,
                N::FromCcsid,
            ],
            &[],
            &[],
        ),
        P::SoapFaultDelete => (&[], &[], &[]),
        P::WsaContextBuild => (
            &[
                N::Channel,
                N::Action,
                N::MessageId,
                N::RelatesUri,
                N::RelatesType,
                N::EprType,
                N::EprField,
                N::EprFrom,
                N::EprLength,
                N::FromCcsid,
                N::FromCodepage,
            ],
            &[],
            &[],
        ),
        P::WsaContextDelete => (&[N::Channel], &[], &[]),
        P::WsaContextGet => (
            &[
                N::Channel,
                N::ContextType,
                N::RelatesIndex,
                N::EprType,
                N::EprField,
                N::EprLength,
                N::IntoCcsid,
                N::IntoCodepage,
            ],
            &[],
            &[
                O::WebAction,
                O::WebMessageId,
                O::WebRelatesUri,
                O::WebRelatesType,
                O::WebEprInto,
                O::WebEprSet,
                O::WebEprLength,
            ],
        ),
        P::WsaEprCreate => (
            &[
                N::Address,
                N::RefParms,
                N::RefParmsLen,
                N::Metadata,
                N::MetadataLen,
                N::EprLength,
                N::FromCcsid,
                N::FromCodepage,
            ],
            &[N::Address],
            &[O::WebEprInto, O::WebEprSet, O::WebEprLength],
        ),
        _ => return true,
    };
    let allowed_outputs = results
        .iter()
        .copied()
        .chain([O::Resp, O::Resp2])
        .collect::<BTreeSet<_>>();
    if !inputs.is_subset(&allowed.iter().copied().collect())
        || !required.iter().all(|name| inputs.contains(name))
        || !outputs.is_subset(&allowed_outputs)
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
    {
        return true;
    }
    if matches!(plan.operation, P::InvokeService)
        && (inputs.contains(&N::Uri) && inputs.contains(&N::UriMap)
            || inputs.contains(&N::Scope) != inputs.contains(&N::ScopeLen))
    {
        return true;
    }
    if matches!(plan.operation, P::SoapFaultCreate | P::SoapFaultAdd) {
        for (data, length) in [
            (N::FaultString, N::FaultStrLen),
            (N::FaultCodeStr, N::FaultCodeLen),
            (N::SubcodeStr, N::SubcodeLen),
            (N::SoapRole, N::RoleLength),
            (N::FaultActor, N::FaultActLen),
            (N::Detail, N::DetailLength),
        ] {
            if inputs.contains(&data) != inputs.contains(&length) {
                return true;
            }
        }
        if plan.operation == P::SoapFaultCreate
            && inputs.contains(&N::FaultCode) == inputs.contains(&N::FaultCodeStr)
        {
            return true;
        }
        if plan.operation == P::SoapFaultAdd
            && !inputs.contains(&N::FaultString)
            && !inputs.contains(&N::SubcodeStr)
        {
            return true;
        }
    }
    if plan.operation == P::WsaContextBuild
        && (![N::Action, N::MessageId, N::RelatesUri, N::EprFrom]
            .iter()
            .any(|name| inputs.contains(name))
            || inputs.contains(&N::EprFrom)
                && (!inputs.contains(&N::EprType) || !inputs.contains(&N::EprField)))
    {
        return true;
    }
    if matches!(
        plan.operation,
        P::WsaContextBuild | P::WsaContextGet | P::WsaEprCreate
    ) {
        if inputs.contains(&N::FromCcsid) && inputs.contains(&N::FromCodepage)
            || inputs.contains(&N::IntoCcsid) && inputs.contains(&N::IntoCodepage)
            || plan.operation == P::WsaContextBuild
                && inputs.contains(&N::EprFrom) != inputs.contains(&N::EprLength)
            || inputs.contains(&N::RelatesType) && !inputs.contains(&N::RelatesUri)
            || inputs.contains(&N::RefParms) != inputs.contains(&N::RefParmsLen)
            || inputs.contains(&N::Metadata) != inputs.contains(&N::MetadataLen)
        {
            return true;
        }
    }
    if matches!(plan.operation, P::WsaContextGet | P::WsaEprCreate) {
        let into = outputs.contains(&O::WebEprInto);
        let set = outputs.contains(&O::WebEprSet);
        if plan.operation == P::WsaContextGet
            && ((inputs.contains(&N::EprType) != (into || set))
                || (inputs.contains(&N::EprField) != (into || set)))
            || into && set
            || plan.operation == P::WsaEprCreate && !into && !set
            || (into || set) != outputs.contains(&O::WebEprLength)
            || into && !inputs.contains(&N::EprLength)
            || plan.operation == P::WsaContextGet
                && !outputs
                    .iter()
                    .any(|name| !matches!(name, O::Resp | O::Resp2 | O::WebEprLength))
        {
            return true;
        }
        if let (Some(CicsOperandValue::Storage(input)), Some(output)) = (
            operand_value(plan, N::EprLength),
            output_target(&plan.outputs, O::WebEprLength),
        ) && input != output
        {
            return true;
        }
    }
    plan.operands.iter().any(|operand| match operand.name {
        N::FaultCode => !matches!(&operand.value, CicsOperandValue::Literal(value)
            if matches!(value.as_slice(), b"CLIENT" | b"SERVER" | b"SENDER" | b"RECEIVER")),
        N::EprType => !matches!(&operand.value, CicsOperandValue::Literal(value)
            if matches!(value.as_slice(), b"TOEPR" | b"REPLYTOEPR" | b"FAULTTOEPR" | b"FROMEPR")),
        N::EprField => !matches!(&operand.value, CicsOperandValue::Literal(value)
            if matches!(value.as_slice(), b"ADDRESS" | b"ALL" | b"METADATA" | b"REFPARMS")),
        N::ContextType => !matches!(&operand.value, CicsOperandValue::Literal(value)
            if matches!(value.as_slice(), b"REQCONTEXT" | b"RESPCONTEXT")),
        N::FaultCodeLen
        | N::FaultStrLen
        | N::RoleLength
        | N::FaultActLen
        | N::DetailLength
        | N::SubcodeLen
        | N::ScopeLen
        | N::EprLength
        | N::RefParmsLen
        | N::MetadataLen
        | N::RelatesIndex
        | N::FromCcsid
        | N::IntoCcsid => !matches!(
            operand.value,
            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
        ),
        _ => !matches!(
            operand.value,
            CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
        ),
    })
}
