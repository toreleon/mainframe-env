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
    let operation = plan.operation;
    let record = matches!(
        operation,
        CicsPlanOperation::IssueAdd
            | CicsPlanOperation::IssueErase
            | CicsPlanOperation::IssueReplace
    );
    let writes = matches!(
        operation,
        CicsPlanOperation::IssueAdd
            | CicsPlanOperation::IssueReplace
            | CicsPlanOperation::IssueSend
    );
    let media = matches!(
        operation,
        CicsPlanOperation::IssueAbort
            | CicsPlanOperation::IssueEnd
            | CicsPlanOperation::IssueSend
            | CicsPlanOperation::IssueWait
    );
    let mut allowed_inputs = BTreeSet::new();
    if operation != CicsPlanOperation::IssueReceive {
        allowed_inputs.extend([
            CicsOperandName::DestId,
            CicsOperandName::DestIdLength,
            CicsOperandName::Volume,
            CicsOperandName::VolumeLength,
        ]);
    }
    if media {
        allowed_inputs.insert(CicsOperandName::Subaddress);
    }
    if writes {
        allowed_inputs.extend([CicsOperandName::From, CicsOperandName::Length]);
    }
    if record {
        allowed_inputs.extend([CicsOperandName::Ridfld, CicsOperandName::NumRec]);
    }
    if matches!(
        operation,
        CicsPlanOperation::IssueErase | CicsPlanOperation::IssueReplace
    ) {
        allowed_inputs.extend([CicsOperandName::KeyLength, CicsOperandName::KeyNumber]);
    }
    if operation == CicsPlanOperation::IssueReceive {
        allowed_inputs.insert(CicsOperandName::Length);
    }
    let mut allowed_outputs = BTreeSet::from([CicsOutputName::Resp, CicsOutputName::Resp2]);
    if operation == CicsPlanOperation::IssueNote {
        allowed_outputs.insert(CicsOutputName::Ridfld);
    }
    if operation == CicsPlanOperation::IssueReceive {
        allowed_outputs.extend([
            CicsOutputName::Into,
            CicsOutputName::SetPointer,
            CicsOutputName::Length,
        ]);
    }
    let mut allowed_options = BTreeSet::from([CicsPlanOption::NoHandle]);
    if record || operation == CicsPlanOperation::IssueSend {
        allowed_options.extend([CicsPlanOption::DefResp, CicsPlanOption::NoWait]);
    }
    if record || operation == CicsPlanOperation::IssueNote {
        allowed_options.insert(CicsPlanOption::Rrn);
    }
    if media {
        allowed_options.extend([
            CicsPlanOption::Console,
            CicsPlanOption::PrintMedium,
            CicsPlanOption::Card,
            CicsPlanOption::WpMedia1,
            CicsPlanOption::WpMedia2,
            CicsPlanOption::WpMedia3,
            CicsPlanOption::WpMedia4,
        ]);
    }
    let media_count = [
        CicsPlanOption::Console,
        CicsPlanOption::PrintMedium,
        CicsPlanOption::Card,
        CicsPlanOption::WpMedia1,
        CicsPlanOption::WpMedia2,
        CicsPlanOption::WpMedia3,
        CicsPlanOption::WpMedia4,
    ]
    .iter()
    .filter(|option| plan.options.contains(option))
    .count();
    !inputs.is_subset(&allowed_inputs)
        || !outputs.is_subset(&allowed_outputs)
        || !plan.options.is_subset(&allowed_options)
        || media_count > 1
        || media_count > 0 && inputs.contains(&CicsOperandName::DestId)
        || inputs.contains(&CicsOperandName::DestIdLength)
            && !inputs.contains(&CicsOperandName::DestId)
        || inputs.contains(&CicsOperandName::VolumeLength)
            && !inputs.contains(&CicsOperandName::Volume)
        || inputs.contains(&CicsOperandName::Subaddress) && media_count == 0
        || inputs.contains(&CicsOperandName::From) != inputs.contains(&CicsOperandName::Length)
            && writes
        || writes && !inputs.contains(&CicsOperandName::From)
        || matches!(
            operation,
            CicsPlanOperation::IssueErase | CicsPlanOperation::IssueReplace
        ) && !inputs.contains(&CicsOperandName::Ridfld)
        || operation == CicsPlanOperation::IssueNote
            && (!outputs.contains(&CicsOutputName::Ridfld)
                || !plan.options.contains(&CicsPlanOption::Rrn))
        || operation == CicsPlanOperation::IssueReceive
            && (!inputs.contains(&CicsOperandName::Length)
                || !outputs.contains(&CicsOutputName::Length)
                || outputs.contains(&CicsOutputName::Into)
                    == outputs.contains(&CicsOutputName::SetPointer))
        || plan.options.contains(&CicsPlanOption::Rrn)
            && (inputs.contains(&CicsOperandName::KeyLength)
                || inputs.contains(&CicsOperandName::KeyNumber))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::DestId | CicsOperandName::Volume => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
            CicsOperandName::From | CicsOperandName::Ridfld => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::Length if operation == CicsPlanOperation::IssueReceive => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            CicsOperandName::Length
            | CicsOperandName::DestIdLength
            | CicsOperandName::Subaddress
            | CicsOperandName::VolumeLength
            | CicsOperandName::NumRec
            | CicsOperandName::KeyLength
            | CicsOperandName::KeyNumber => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            _ => true,
        })
}
