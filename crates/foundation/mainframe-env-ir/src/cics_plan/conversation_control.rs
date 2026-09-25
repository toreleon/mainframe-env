//! Checked v0.9 conversation extraction and positioning plan shapes.

use super::{
    CicsEffectPlan, CicsOperandName as I, CicsOutputName as O, CicsPlanOperation as P,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) const fn is_operation(operation: P) -> bool {
    matches!(
        operation,
        P::ExtractAttach
            | P::ExtractAttributes
            | P::GdsExtractAttributes
            | P::ExtractLogonMsg
            | P::ExtractProcess
            | P::GdsExtractProcess
            | P::ExtractTct
            | P::Point
    )
}

pub(super) const fn allowed_output(operation: P, output: O) -> bool {
    if matches!(output, O::Resp | O::Resp2) {
        return true;
    }
    match operation {
        P::ExtractAttach => matches!(
            output,
            O::AttachProcess
                | O::AttachResource
                | O::AttachReturnProcess
                | O::AttachReturnResource
                | O::AttachQueue
                | O::AttachIuType
                | O::AttachDataStream
                | O::AttachRecordFormat
        ),
        P::ExtractAttributes => matches!(output, O::ConversationState),
        P::GdsExtractAttributes => matches!(
            output,
            O::ConversationState | O::ConversationData | O::ConversationRetCode
        ),
        P::ExtractLogonMsg => matches!(output, O::LogonInto | O::LogonSet | O::LogonLength),
        P::ExtractProcess => matches!(
            output,
            O::ProcessName | O::ProcessLength | O::SyncLevel | O::PipList | O::PipLength
        ),
        P::GdsExtractProcess => matches!(
            output,
            O::ProcessName
                | O::ProcessLength
                | O::SyncLevel
                | O::PipList
                | O::PipLength
                | O::ConversationRetCode
        ),
        P::ExtractTct => matches!(output, O::TctSysId | O::TctTermId),
        P::Point => false,
        _ => false,
    }
}

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<I>,
    outputs: &BTreeSet<O>,
) -> bool {
    if plan
        .options
        .iter()
        .any(|option| *option != CicsPlanOption::NoHandle)
        || inputs
            .iter()
            .any(|input| !allowed_input(plan.operation, *input))
        || outputs
            .iter()
            .any(|output| !allowed_output(plan.operation, *output))
    {
        return true;
    }
    let selectors = [
        I::ConversationAttachId,
        I::ConversationConvid,
        I::ConversationSession,
    ]
    .iter()
    .filter(|name| inputs.contains(name))
    .count();
    if selectors > 1 {
        return true;
    }
    match plan.operation {
        P::ExtractAttach => false,
        P::ExtractAttributes => !outputs.contains(&O::ConversationState),
        P::GdsExtractAttributes => {
            !inputs.contains(&I::ConversationConvid)
                || !outputs.contains(&O::ConversationData)
                || !outputs.contains(&O::ConversationRetCode)
        }
        P::ExtractLogonMsg => {
            !outputs.contains(&O::LogonLength)
                || (outputs.contains(&O::LogonInto) == outputs.contains(&O::LogonSet))
        }
        P::ExtractProcess => process_shape(inputs, outputs, false),
        P::GdsExtractProcess => {
            !inputs.contains(&I::ConversationConvid)
                || !outputs.contains(&O::ConversationRetCode)
                || process_shape(inputs, outputs, true)
        }
        P::ExtractTct => {
            !inputs.contains(&I::ConversationNetName)
                || (outputs.contains(&O::TctSysId) == outputs.contains(&O::TctTermId))
        }
        P::Point => false,
        _ => true,
    }
}

fn process_shape(inputs: &BTreeSet<I>, outputs: &BTreeSet<O>, _gds: bool) -> bool {
    outputs.contains(&O::ProcessName) && !outputs.contains(&O::ProcessLength)
        || inputs.contains(&I::ConversationMaxProcLen) && !outputs.contains(&O::ProcessName)
        || outputs.contains(&O::PipList) != outputs.contains(&O::PipLength)
}

const fn allowed_input(operation: P, input: I) -> bool {
    match operation {
        P::ExtractAttach => matches!(
            input,
            I::ConversationAttachId | I::ConversationConvid | I::ConversationSession
        ),
        P::ExtractAttributes | P::Point => {
            matches!(input, I::ConversationConvid | I::ConversationSession)
        }
        P::GdsExtractAttributes | P::GdsExtractProcess => {
            matches!(input, I::ConversationConvid | I::ConversationMaxProcLen)
        }
        P::ExtractLogonMsg => false,
        P::ExtractProcess => matches!(
            input,
            I::ConversationConvid | I::ConversationSession | I::ConversationMaxProcLen
        ),
        P::ExtractTct => matches!(input, I::ConversationNetName),
        _ => false,
    }
}
