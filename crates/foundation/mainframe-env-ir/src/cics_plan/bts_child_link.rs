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
    let valid = match plan.operation {
        CicsPlanOperation::FetchAny => {
            outputs.contains(&CicsOutputName::BtsAny)
                && outputs.contains(&CicsOutputName::BtsChildCompStatus)
                && inputs.is_subset(&BTreeSet::from([CicsOperandName::BtsTimeout]))
                && plan.options.is_subset(&BTreeSet::from([
                    CicsPlanOption::NoHandle,
                    CicsPlanOption::BtsNoSuspend,
                ]))
        }
        CicsPlanOperation::FetchChild => {
            outputs.contains(&CicsOutputName::BtsChildCompStatus)
                && inputs.contains(&CicsOperandName::BtsChild)
                && inputs.is_subset(&BTreeSet::from([
                    CicsOperandName::BtsChild,
                    CicsOperandName::BtsTimeout,
                ]))
                && plan.options.is_subset(&BTreeSet::from([
                    CicsPlanOption::NoHandle,
                    CicsPlanOption::BtsNoSuspend,
                ]))
        }
        CicsPlanOperation::FreeChild => {
            inputs == &BTreeSet::from([CicsOperandName::BtsChild])
                && plan
                    .options
                    .is_subset(&BTreeSet::from([CicsPlanOption::NoHandle]))
        }
        CicsPlanOperation::LinkActivity => {
            inputs.contains(&CicsOperandName::BtsLinkActivity)
                && inputs.is_subset(&BTreeSet::from([
                    CicsOperandName::BtsLinkActivity,
                    CicsOperandName::BtsLinkInputEvent,
                ]))
                && plan
                    .options
                    .is_subset(&BTreeSet::from([CicsPlanOption::NoHandle]))
        }
        CicsPlanOperation::LinkAcqActivity => {
            inputs.is_subset(&BTreeSet::from([CicsOperandName::BtsLinkInputEvent]))
                && plan.options.is_subset(&BTreeSet::from([
                    CicsPlanOption::NoHandle,
                    CicsPlanOption::BtsAcqActivity,
                ]))
                && plan.options.contains(&CicsPlanOption::BtsAcqActivity)
        }
        CicsPlanOperation::LinkAcqProcess => {
            inputs.is_subset(&BTreeSet::from([CicsOperandName::BtsLinkInputEvent]))
                && plan.options.is_subset(&BTreeSet::from([
                    CicsPlanOption::NoHandle,
                    CicsPlanOption::BtsAcqProcess,
                ]))
                && plan.options.contains(&CicsPlanOption::BtsAcqProcess)
        }
        _ => false,
    };
    !valid
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::BtsTimeout => !matches!(
                operand.value,
                CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
            ),
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
        || plan.options.contains(&CicsPlanOption::BtsNoSuspend)
            && inputs.contains(&CicsOperandName::BtsTimeout)
        || matches!(
            plan.operation,
            CicsPlanOperation::FreeChild
                | CicsPlanOperation::LinkActivity
                | CicsPlanOperation::LinkAcqActivity
                | CicsPlanOperation::LinkAcqProcess
        ) && outputs
            .iter()
            .any(|name| !matches!(name, CicsOutputName::Resp | CicsOutputName::Resp2))
}
