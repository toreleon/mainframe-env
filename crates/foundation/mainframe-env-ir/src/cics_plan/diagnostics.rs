use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_trace_num_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::TraceNum)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::TraceNum,
            CicsOperandName::TraceFrom,
            CicsOperandName::TraceFromLength,
            CicsOperandName::TraceResource,
        ]))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::TraceNum => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::TraceFrom | CicsOperandName::TraceFromLength => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::TraceResource => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::TraceException
            )
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_monitor_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.contains(&CicsOperandName::MonitorPoint)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::MonitorPoint,
            CicsOperandName::MonitorEntryName,
            CicsOperandName::MonitorData1,
            CicsOperandName::MonitorData2,
        ]))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::MonitorPoint => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::MonitorEntryName => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::MonitorData1 | CicsOperandName::MonitorData2 => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            _ => true,
        })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_dump_transaction_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let segments = [
        CicsOperandName::DumpSegmentList,
        CicsOperandName::DumpLengthList,
        CicsOperandName::DumpNumSegments,
    ]
    .iter()
    .filter(|name| inputs.contains(name))
    .count();
    !inputs.contains(&CicsOperandName::DumpCode)
        || !inputs.is_subset(&BTreeSet::from([
            CicsOperandName::DumpCode,
            CicsOperandName::DumpFrom,
            CicsOperandName::DumpLength,
            CicsOperandName::DumpFlength,
            CicsOperandName::DumpSegmentList,
            CicsOperandName::DumpLengthList,
            CicsOperandName::DumpNumSegments,
        ]))
        || inputs.contains(&CicsOperandName::DumpLength)
            && inputs.contains(&CicsOperandName::DumpFlength)
        || (inputs.contains(&CicsOperandName::DumpLength)
            || inputs.contains(&CicsOperandName::DumpFlength))
            && !inputs.contains(&CicsOperandName::DumpFrom)
        || segments != 0 && segments != 3
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::DumpCode => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::DumpFrom
            | CicsOperandName::DumpSegmentList
            | CicsOperandName::DumpLengthList
            | CicsOperandName::DumpNumSegments => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::DumpLength | CicsOperandName::DumpFlength => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || outputs.iter().any(|output| {
            !matches!(
                output,
                CicsOutputName::DumpId | CicsOutputName::Resp | CicsOutputName::Resp2
            )
        })
        || super::option_shape::has_unsupported(plan)
}

pub(super) fn invalid_dump_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    !inputs.is_subset(&BTreeSet::from([
        CicsOperandName::DumpCode,
        CicsOperandName::DumpFrom,
        CicsOperandName::DumpLength,
        CicsOperandName::DumpFlength,
    ])) || inputs.contains(&CicsOperandName::DumpLength)
        && inputs.contains(&CicsOperandName::DumpFlength)
        || (inputs.contains(&CicsOperandName::DumpLength)
            || inputs.contains(&CicsOperandName::DumpFlength))
            && !inputs.contains(&CicsOperandName::DumpFrom)
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::DumpCode => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::DumpFrom => !matches!(operand.value, CicsOperandValue::Storage(_)),
            CicsOperandName::DumpLength | CicsOperandName::DumpFlength => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            _ => true,
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
        || super::option_shape::has_unsupported(plan)
}
