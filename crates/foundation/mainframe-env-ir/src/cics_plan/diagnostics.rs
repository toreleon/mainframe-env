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
