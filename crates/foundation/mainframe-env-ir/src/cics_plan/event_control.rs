use super::{CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption};
use std::collections::BTreeSet;

pub(super) fn invalid_define_input_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs.len() != 1
        || !inputs.contains(&CicsOperandName::Event)
        || plan.operands.iter().any(|operand| {
            operand.name != CicsOperandName::Event
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
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_define_composite_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let all = plan.options.contains(&CicsPlanOption::EventAnd);
    let any = plan.options.contains(&CicsPlanOption::EventOr);
    !inputs.contains(&CicsOperandName::Event)
        || inputs.len() > 9
        || all == any
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::EventAnd | CicsPlanOption::EventOr
            )
        })
        || plan.operands.iter().any(|operand| {
            !matches!(
                operand.name,
                CicsOperandName::Event
                    | CicsOperandName::SubEvent1
                    | CicsOperandName::SubEvent2
                    | CicsOperandName::SubEvent3
                    | CicsOperandName::SubEvent4
                    | CicsOperandName::SubEvent5
                    | CicsOperandName::SubEvent6
                    | CicsOperandName::SubEvent7
                    | CicsOperandName::SubEvent8
            ) || !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_membership_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    inputs.len() != 2
        || !inputs.contains(&CicsOperandName::Event)
        || !inputs.contains(&CicsOperandName::SubEvent)
        || plan.operands.iter().any(|operand| {
            !matches!(
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
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_timer_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    if !inputs.contains(&CicsOperandName::Timer)
        || !matches!(
            plan.operands.iter().find(|operand| operand.name == CicsOperandName::Timer),
            Some(operand) if matches!(operand.value, CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_))
        )
    {
        return true;
    }
    match plan.operation {
        super::CicsPlanOperation::DefineTimer => {
            let after = plan.options.contains(&CicsPlanOption::TimerAfter);
            let at = plan.options.contains(&CicsPlanOption::TimerAt);
            after == at
                || plan.options.contains(&CicsPlanOption::TimerOn) && !at
                || ![
                    CicsOperandName::TimerDays,
                    CicsOperandName::TimerHours,
                    CicsOperandName::TimerMinutes,
                    CicsOperandName::TimerSeconds,
                ]
                .iter()
                .any(|name| inputs.contains(name))
                || at && inputs.contains(&CicsOperandName::TimerDays)
                || inputs.iter().any(|name| {
                    !matches!(
                        name,
                        CicsOperandName::Timer
                            | CicsOperandName::Event
                            | CicsOperandName::TimerDays
                            | CicsOperandName::TimerHours
                            | CicsOperandName::TimerMinutes
                            | CicsOperandName::TimerSeconds
                            | CicsOperandName::TimerYear
                            | CicsOperandName::TimerMonth
                            | CicsOperandName::TimerDayOfMonth
                            | CicsOperandName::TimerDayOfYear
                    )
                })
                || outputs
                    .iter()
                    .any(|name| !matches!(name, CicsOutputName::Resp | CicsOutputName::Resp2))
        }
        super::CicsPlanOperation::CheckTimer => {
            inputs.len() != 1 || !outputs.contains(&CicsOutputName::TimerStatus)
        }
        super::CicsPlanOperation::DeleteTimer => inputs.len() != 1,
        super::CicsPlanOperation::ForceTimer => {
            inputs.len() != 1
                || plan.options.contains(&CicsPlanOption::AcqActivity)
                    && plan.options.contains(&CicsPlanOption::AcqProcess)
        }
        _ => true,
    }
}

pub(super) fn invalid_retrieve_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    if plan
        .options
        .iter()
        .any(|option| *option != CicsPlanOption::NoHandle)
    {
        return true;
    }
    match plan.operation {
        super::CicsPlanOperation::RetrieveReattachEvent => {
            !inputs.is_empty()
                || !outputs.contains(&CicsOutputName::EventName)
                || !outputs.contains(&CicsOutputName::EventType)
        }
        super::CicsPlanOperation::RetrieveSubevent | super::CicsPlanOperation::TestEvent => {
            inputs.len() != 1
                || !inputs.contains(&CicsOperandName::Event)
                || !matches!(
                    plan.operands.first(),
                    Some(operand) if matches!(operand.value, CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_))
                )
                || if plan.operation == super::CicsPlanOperation::RetrieveSubevent {
                    !outputs.contains(&CicsOutputName::SubEventName)
                        || !outputs.contains(&CicsOutputName::EventType)
                } else {
                    !outputs.contains(&CicsOutputName::FireStatus)
                }
        }
        _ => true,
    }
}

pub(super) fn invalid_signal_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    if !inputs.contains(&CicsOperandName::Event)
        || inputs.len() > 4
        || inputs.iter().any(|name| {
            !matches!(
                name,
                CicsOperandName::Event
                    | CicsOperandName::SignalFrom
                    | CicsOperandName::SignalFromLength
                    | CicsOperandName::SignalFromChannel
            )
        })
        || inputs.contains(&CicsOperandName::SignalFrom)
            && inputs.contains(&CicsOperandName::SignalFromChannel)
        || inputs.contains(&CicsOperandName::SignalFromLength)
            && !inputs.contains(&CicsOperandName::SignalFrom)
        || plan
            .options
            .iter()
            .any(|option| *option != CicsPlanOption::NoHandle)
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
    {
        return true;
    }
    plan.operands.iter().any(|operand| match operand.name {
        CicsOperandName::Event | CicsOperandName::SignalFromChannel => !matches!(
            operand.value,
            CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
        ),
        CicsOperandName::SignalFrom => !matches!(operand.value, CicsOperandValue::Storage(_)),
        CicsOperandName::SignalFromLength => !matches!(
            operand.value,
            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
        ),
        _ => true,
    })
}
