//! Closed typed shape for full-BMS ROUTE.

use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Errterm,
        CicsOperandName::RouteTitle,
        CicsOperandName::RouteList,
        CicsOperandName::Opclass,
        CicsOperandName::ReqId,
        CicsOperandName::Ldc,
        CicsOperandName::Interval,
        CicsOperandName::StartTime,
        CicsOperandName::Hours,
        CicsOperandName::Minutes,
        CicsOperandName::Seconds,
    ]);
    let allowed_options = BTreeSet::from([
        CicsPlanOption::NoHandle,
        CicsPlanOption::After,
        CicsPlanOption::At,
        CicsPlanOption::Nleom,
    ]);
    let timing_count = usize::from(inputs.contains(&CicsOperandName::Interval))
        + usize::from(inputs.contains(&CicsOperandName::StartTime))
        + usize::from(plan.options.contains(&CicsPlanOption::After))
        + usize::from(plan.options.contains(&CicsPlanOption::At));
    let has_units = [
        CicsOperandName::Hours,
        CicsOperandName::Minutes,
        CicsOperandName::Seconds,
    ]
    .iter()
    .any(|name| inputs.contains(name));
    let unit_form =
        plan.options.contains(&CicsPlanOption::After) || plan.options.contains(&CicsPlanOption::At);
    !inputs.is_subset(&allowed_inputs)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || !plan.options.is_subset(&allowed_options)
        || timing_count > 1
        || has_units != unit_form
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::RouteTitle | CicsOperandName::RouteList | CicsOperandName::Opclass => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::Errterm | CicsOperandName::ReqId | CicsOperandName::Ldc => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
        })
}
