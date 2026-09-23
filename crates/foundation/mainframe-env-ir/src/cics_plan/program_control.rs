use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOption,
    output_target,
};
use std::collections::BTreeSet;

pub(super) fn invalid_link_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    invalid_transfer_shape(plan, inputs, outputs, true)
}

pub(super) fn invalid_invoke_application_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Application,
        CicsOperandName::Platform,
        CicsOperandName::ApplicationOperation,
        CicsOperandName::MajorVersion,
        CicsOperandName::MinorVersion,
        CicsOperandName::Commarea,
        CicsOperandName::Length,
        CicsOperandName::Channel,
    ]);
    let named = |name| plan.operands.iter().find(|operand| operand.name == name);
    let valid_name = |name| {
        named(name).is_some_and(|operand| {
            matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_application_name(bytes))
                || matches!(operand.value, CicsOperandValue::Storage(_))
        })
    };
    let versions = (
        inputs.contains(&CicsOperandName::MajorVersion),
        inputs.contains(&CicsOperandName::MinorVersion),
    );
    let commarea = named(CicsOperandName::Commarea);
    let commarea_output = output_target(&plan.outputs, CicsOutputName::Commarea);
    !inputs.is_subset(&allowed_inputs)
        || !valid_name(CicsOperandName::Application)
        || !valid_name(CicsOperandName::ApplicationOperation)
        || inputs.contains(&CicsOperandName::Platform)
            && !valid_name(CicsOperandName::Platform)
        || inputs.contains(&CicsOperandName::Channel)
            && named(CicsOperandName::Channel).is_none_or(|operand| {
                !matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_channel_name(bytes))
                    && !matches!(operand.value, CicsOperandValue::Storage(_))
            })
        || versions.0 != versions.1
        || versions.0
            && [CicsOperandName::MajorVersion, CicsOperandName::MinorVersion]
                .into_iter()
                .any(|name| {
                    named(name).is_none_or(|operand| {
                        !matches!(
                            operand.value,
                            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
                        )
                    })
                })
        || !versions.0
            && plan.options.iter().any(|option| {
                matches!(
                    option,
                    CicsPlanOption::ExactMatch | CicsPlanOption::Minimum
                )
            })
        || plan.options.contains(&CicsPlanOption::ExactMatch)
            && plan.options.contains(&CicsPlanOption::Minimum)
        || inputs.contains(&CicsOperandName::Commarea)
            && inputs.contains(&CicsOperandName::Channel)
        || commarea.is_some_and(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
        || match commarea.map(|operand| &operand.value) {
            Some(CicsOperandValue::Storage(slot)) => commarea_output != Some(slot),
            Some(_) => true,
            None => commarea_output.is_some(),
        }
        || invalid_commarea_length(plan)
        || plan.options.iter().any(|option| {
            !matches!(
                option,
                CicsPlanOption::NoHandle
                    | CicsPlanOption::ExactMatch
                    | CicsPlanOption::Minimum
            )
        })
        || outputs.contains(&CicsOutputName::Into)
}

pub(super) fn invalid_load_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Program,
        CicsOperandName::LoadSet,
        CicsOperandName::Entry,
        CicsOperandName::LoadLength,
        CicsOperandName::LoadFlength,
    ]);
    let program = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Program);
    !inputs.contains(&CicsOperandName::Program)
        || !inputs.is_subset(&allowed_inputs)
        || inputs.contains(&CicsOperandName::LoadLength)
            && inputs.contains(&CicsOperandName::LoadFlength)
        || program.is_none_or(|operand| {
            !matches!(&operand.value, CicsOperandValue::Literal(bytes) if valid_program_name(bytes))
                && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || plan.operands.iter().any(|operand| {
            matches!(
                operand.name,
                CicsOperandName::LoadSet
                    | CicsOperandName::Entry
                    | CicsOperandName::LoadLength
                    | CicsOperandName::LoadFlength
            ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Hold))
        || outputs
            .iter()
            .any(|output| !matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2))
}

pub(super) fn invalid_xctl_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    invalid_transfer_shape(plan, inputs, outputs, false)
}

pub(super) fn invalid_return_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::TransId,
        CicsOperandName::Commarea,
        CicsOperandName::Length,
    ]);
    let transid = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::TransId);
    !inputs.is_subset(&allowed_inputs)
        || inputs.contains(&CicsOperandName::Commarea)
            && !inputs.contains(&CicsOperandName::TransId)
        || transid.is_some_and(|operand| {
            !matches!(
                &operand.value,
                CicsOperandValue::Literal(bytes) if valid_transaction_name(bytes)
            ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || plan.operands.iter().any(|operand| {
            operand.name == CicsOperandName::Commarea
                && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || invalid_commarea_length(plan)
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || outputs.contains(&CicsOutputName::Into)
}

fn invalid_transfer_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
    returns_commarea: bool,
) -> bool {
    let allowed_inputs = BTreeSet::from([
        CicsOperandName::Program,
        CicsOperandName::Commarea,
        CicsOperandName::Length,
        CicsOperandName::DataLength,
    ]);
    let program = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Program);
    let commarea = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Commarea);
    let commarea_output = output_target(&plan.outputs, CicsOutputName::Commarea);
    !inputs.contains(&CicsOperandName::Program)
        || !inputs.is_subset(&allowed_inputs)
        || program.is_none_or(|operand| {
            !matches!(
                &operand.value,
                CicsOperandValue::Literal(bytes) if valid_program_name(bytes)
            ) && !matches!(operand.value, CicsOperandValue::Storage(_))
        })
        || commarea.is_some_and(|operand| !matches!(operand.value, CicsOperandValue::Storage(_)))
        || match commarea.map(|operand| &operand.value) {
            Some(CicsOperandValue::Storage(slot)) if returns_commarea => {
                commarea_output != Some(slot)
            }
            Some(CicsOperandValue::Storage(_)) => commarea_output.is_some(),
            Some(_) => true,
            None => commarea_output.is_some(),
        }
        || invalid_commarea_length(plan)
        || invalid_data_length(plan)
        || plan
            .options
            .iter()
            .any(|option| !matches!(option, CicsPlanOption::NoHandle))
        || outputs.contains(&CicsOutputName::Into)
}

fn invalid_data_length(plan: &CicsEffectPlan) -> bool {
    let Some(value) = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::DataLength)
        .map(|operand| &operand.value)
    else {
        return false;
    };
    plan.operation != super::CicsPlanOperation::Link
        || !plan
            .operands
            .iter()
            .any(|operand| operand.name == CicsOperandName::Commarea)
        || !plan
            .operands
            .iter()
            .any(|operand| operand.name == CicsOperandName::Length)
        || !matches!(
            value,
            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
        )
}

fn invalid_commarea_length(plan: &CicsEffectPlan) -> bool {
    let commarea = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Commarea);
    let length = plan
        .operands
        .iter()
        .find(|operand| operand.name == CicsOperandName::Length);
    match (
        commarea.map(|operand| &operand.value),
        length.map(|operand| &operand.value),
    ) {
        (_, None) => false,
        (Some(CicsOperandValue::Storage(commarea)), Some(CicsOperandValue::LengthOf(length))) => {
            commarea != length
        }
        (Some(CicsOperandValue::Storage(_)), Some(value)) => !matches!(
            value,
            CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
        ),
        _ => true,
    }
}

fn valid_program_name(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 1..=8) && bytes.iter().all(u8::is_ascii_alphanumeric)
}

fn valid_application_name(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 1..=64)
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'#' | b'@' | b'-')
        })
}

fn valid_channel_name(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 1..=16) && !bytes.iter().any(u8::is_ascii_whitespace)
}

fn valid_transaction_name(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 1..=4)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}
