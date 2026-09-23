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
    if plan.operation == CicsPlanOperation::SendControl {
        let allowed_inputs = BTreeSet::from([
            CicsOperandName::ControlCursor,
            CicsOperandName::Msr,
            CicsOperandName::Outpartn,
            CicsOperandName::Actpartn,
            CicsOperandName::Ldc,
            CicsOperandName::ReqId,
        ]);
        let allowed_outputs = BTreeSet::from([
            CicsOutputName::SetPointer,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]);
        let allowed_options = BTreeSet::from([
            CicsPlanOption::Accum,
            CicsPlanOption::Formfeed,
            CicsPlanOption::DefaultScreen,
            CicsPlanOption::AlternateScreen,
            CicsPlanOption::Erase,
            CicsPlanOption::EraseAup,
            CicsPlanOption::Print,
            CicsPlanOption::FreeKb,
            CicsPlanOption::Alarm,
            CicsPlanOption::Frset,
            CicsPlanOption::Paging,
            CicsPlanOption::Terminal,
            CicsPlanOption::Wait,
            CicsPlanOption::Last,
            CicsPlanOption::Honeom,
            CicsPlanOption::L40,
            CicsPlanOption::L64,
            CicsPlanOption::L80,
            CicsPlanOption::NoHandle,
        ]);
        return !inputs.is_subset(&allowed_inputs)
            || !outputs.is_subset(&allowed_outputs)
            || !plan.options.is_subset(&allowed_options)
            || [
                plan.options.contains(&CicsPlanOption::Terminal),
                plan.options.contains(&CicsPlanOption::Paging),
                outputs.contains(&CicsOutputName::SetPointer),
            ]
            .into_iter()
            .filter(|value| *value)
            .count()
                > 1
            || [
                CicsPlanOption::L40,
                CicsPlanOption::L64,
                CicsPlanOption::L80,
            ]
            .into_iter()
            .filter(|value| plan.options.contains(value))
            .count()
                > 1
            || plan.options.contains(&CicsPlanOption::DefaultScreen)
                && plan.options.contains(&CicsPlanOption::AlternateScreen)
            || plan.operands.iter().any(|operand| match operand.name {
                CicsOperandName::ControlCursor => !matches!(
                    operand.value,
                    CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
                ),
                CicsOperandName::Msr => !matches!(
                    operand.value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                ),
                CicsOperandName::Outpartn
                | CicsOperandName::Actpartn
                | CicsOperandName::Ldc
                | CicsOperandName::ReqId => !matches!(
                    operand.value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                ),
                _ => true,
            });
    }
    if plan.operation == CicsPlanOperation::SendPage {
        let allowed_inputs = BTreeSet::from([
            CicsOperandName::TransId,
            CicsOperandName::Trailer,
            CicsOperandName::Fmhparm,
        ]);
        let allowed_outputs = BTreeSet::from([
            CicsOutputName::SetPointer,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]);
        let allowed_options = BTreeSet::from([
            CicsPlanOption::ReleasePage,
            CicsPlanOption::RetainPage,
            CicsPlanOption::Autopage,
            CicsPlanOption::CurrentPage,
            CicsPlanOption::AllPages,
            CicsPlanOption::NoAutopage,
            CicsPlanOption::OperPurge,
            CicsPlanOption::Last,
            CicsPlanOption::NoHandle,
        ]);
        return !inputs.is_subset(&allowed_inputs)
            || !outputs.is_subset(&allowed_outputs)
            || !plan.options.is_subset(&allowed_options)
            || plan.options.contains(&CicsPlanOption::ReleasePage)
                && plan.options.contains(&CicsPlanOption::RetainPage)
            || plan.options.contains(&CicsPlanOption::Autopage)
                && plan.options.contains(&CicsPlanOption::NoAutopage)
            || inputs.contains(&CicsOperandName::TransId)
                && !plan.options.contains(&CicsPlanOption::ReleasePage)
            || plan.operands.iter().any(|operand| match operand.name {
                CicsOperandName::TransId | CicsOperandName::Fmhparm => !matches!(
                    operand.value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                ),
                CicsOperandName::Trailer => !matches!(operand.value, CicsOperandValue::Storage(_)),
                _ => true,
            });
    }
    if plan.operation == CicsPlanOperation::ReceivePartn {
        return !inputs.is_subset(&BTreeSet::from([CicsOperandName::Length]))
            || !outputs.is_subset(&BTreeSet::from([
                CicsOutputName::Partn,
                CicsOutputName::Into,
                CicsOutputName::SetPointer,
                CicsOutputName::Length,
                CicsOutputName::Resp,
                CicsOutputName::Resp2,
            ]))
            || !outputs.contains(&CicsOutputName::Partn)
            || outputs.contains(&CicsOutputName::Into)
                && outputs.contains(&CicsOutputName::SetPointer)
            || outputs.contains(&CicsOutputName::Into)
                != inputs.contains(&CicsOperandName::Length)
            || outputs.contains(&CicsOutputName::Into)
                != outputs.contains(&CicsOutputName::Length)
            || plan.operands.iter().any(|operand| {
                operand.name != CicsOperandName::Length
                    || !matches!(operand.value, CicsOperandValue::Storage(_))
            })
            || plan
                .options
                .iter()
                .any(|option| !matches!(option, CicsPlanOption::AsIs | CicsPlanOption::NoHandle));
    }
    if plan.operation == CicsPlanOperation::SendPartnset {
        return !inputs.is_subset(&BTreeSet::from([CicsOperandName::Partnset]))
            || plan.operands.iter().any(|operand| {
                operand.name != CicsOperandName::Partnset
                    || !matches!(
                        operand.value,
                        CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                    )
            })
            || plan
                .options
                .iter()
                .any(|option| *option != CicsPlanOption::NoHandle)
            || outputs
                .iter()
                .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2));
    }
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Map,
        CicsOperandName::Mapset,
        CicsOperandName::From,
        CicsOperandName::Length,
    ]);
    let required = match plan.operation {
        CicsPlanOperation::ReceiveMap => BTreeSet::from([CicsOperandName::Map]),
        CicsPlanOperation::SendMap => BTreeSet::from([CicsOperandName::Map]),
        CicsPlanOperation::SendText => BTreeSet::from([CicsOperandName::From]),
        _ => return true,
    };
    !inputs.is_subset(&allowed_inputs)
        || !required.is_subset(inputs)
        || (plan.operation == CicsPlanOperation::ReceiveMap
            && inputs.iter().any(|name| {
                !matches!(
                    name,
                    CicsOperandName::Map
                        | CicsOperandName::Mapset
                        | CicsOperandName::From
                        | CicsOperandName::Length
                )
            }))
        || (plan.operation == CicsPlanOperation::SendText
            && inputs
                .iter()
                .any(|name| !matches!(name, CicsOperandName::From | CicsOperandName::Length)))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::Map | CicsOperandName::Mapset => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::From => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::Length => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => true,
        })
        || match (
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::From),
            plan.operands
                .iter()
                .find(|operand| operand.name == CicsOperandName::Length),
        ) {
            (None, Some(_)) => true,
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
            (Some(_), Some(_)) => false,
            (_, None) => false,
        }
        || (plan.operation != CicsPlanOperation::ReceiveMap
            && outputs.contains(&CicsOutputName::Into))
        || plan.options.iter().any(|option| match option {
            CicsPlanOption::NoHandle => false,
            CicsPlanOption::Erase => !matches!(
                plan.operation,
                CicsPlanOperation::SendMap | CicsPlanOperation::SendText
            ),
            CicsPlanOption::Cursor => plan.operation != CicsPlanOperation::SendMap,
            CicsPlanOption::FreeKb => !matches!(
                plan.operation,
                CicsPlanOperation::SendMap | CicsPlanOperation::SendText
            ),
            CicsPlanOption::MapOnly => {
                plan.operation != CicsPlanOperation::SendMap
                    || inputs.contains(&CicsOperandName::From)
                    || inputs.contains(&CicsOperandName::Length)
                    || plan.options.contains(&CicsPlanOption::DataOnly)
            }
            CicsPlanOption::DataOnly => {
                plan.operation != CicsPlanOperation::SendMap
                    || !inputs.contains(&CicsOperandName::From)
                    || plan.options.contains(&CicsPlanOption::MapOnly)
            }
            CicsPlanOption::Terminal => {
                plan.operation != CicsPlanOperation::ReceiveMap
                    || inputs.contains(&CicsOperandName::From)
            }
            _ => true,
        })
}
