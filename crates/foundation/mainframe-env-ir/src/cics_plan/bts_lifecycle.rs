//! Exact MCEP shapes for the assigned BTS lifecycle command rows.

use super::{
    CicsEffectPlan, CicsOperandName, CicsOperandValue, CicsOutputName, CicsPlanOperation,
    CicsPlanOption,
};
use std::collections::BTreeSet;

pub(super) const fn is_bts(operation: CicsPlanOperation) -> bool {
    matches!(
        operation,
        CicsPlanOperation::AcquireActivityId
            | CicsPlanOperation::AcquireProcess
            | CicsPlanOperation::CancelAcqActivity
            | CicsPlanOperation::CancelAcqProcess
            | CicsPlanOperation::CancelActivity
            | CicsPlanOperation::CheckAcqActivity
            | CicsPlanOperation::CheckAcqProcess
            | CicsPlanOperation::CheckActivity
            | CicsPlanOperation::DefineActivity
            | CicsPlanOperation::DefineProcess
            | CicsPlanOperation::DeleteActivity
            | CicsPlanOperation::ResetAcqProcess
            | CicsPlanOperation::ResetActivity
            | CicsPlanOperation::ResumeAcqActivity
            | CicsPlanOperation::ResumeAcqProcess
            | CicsPlanOperation::ResumeActivity
            | CicsPlanOperation::RunAcqActivity
            | CicsPlanOperation::RunAcqProcess
            | CicsPlanOperation::RunActivity
            | CicsPlanOperation::RunTransId
            | CicsPlanOperation::SuspendAcqActivity
            | CicsPlanOperation::SuspendAcqProcess
            | CicsPlanOperation::SuspendActivity
    )
}

pub(super) const fn allowed_option(operation: CicsPlanOperation, option: CicsPlanOption) -> bool {
    if matches!(option, CicsPlanOption::NoHandle) {
        return true;
    }
    match operation {
        CicsPlanOperation::DefineProcess => matches!(option, CicsPlanOption::NoCheck),
        CicsPlanOperation::CancelAcqActivity
        | CicsPlanOperation::CheckAcqActivity
        | CicsPlanOperation::ResumeAcqActivity
        | CicsPlanOperation::SuspendAcqActivity => {
            matches!(option, CicsPlanOption::AcqActivity)
        }
        CicsPlanOperation::CancelAcqProcess
        | CicsPlanOperation::CheckAcqProcess
        | CicsPlanOperation::ResetAcqProcess
        | CicsPlanOperation::ResumeAcqProcess
        | CicsPlanOperation::SuspendAcqProcess => {
            matches!(option, CicsPlanOption::AcqProcess)
        }
        CicsPlanOperation::RunAcqActivity => matches!(
            option,
            CicsPlanOption::AcqActivity
                | CicsPlanOption::BtsSynchronous
                | CicsPlanOption::BtsAsynchronous
        ),
        CicsPlanOperation::RunAcqProcess => matches!(
            option,
            CicsPlanOption::AcqProcess
                | CicsPlanOption::BtsSynchronous
                | CicsPlanOption::BtsAsynchronous
        ),
        CicsPlanOperation::RunActivity => matches!(
            option,
            CicsPlanOption::BtsSynchronous | CicsPlanOption::BtsAsynchronous
        ),
        _ => false,
    }
}

pub(super) const fn allowed_output(operation: CicsPlanOperation, output: CicsOutputName) -> bool {
    if matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2) {
        return true;
    }
    match operation {
        CicsPlanOperation::DefineActivity => {
            matches!(output, CicsOutputName::BtsActivityId)
        }
        CicsPlanOperation::CheckAcqActivity
        | CicsPlanOperation::CheckAcqProcess
        | CicsPlanOperation::CheckActivity => matches!(
            output,
            CicsOutputName::BtsCompStatus
                | CicsOutputName::BtsMode
                | CicsOutputName::BtsSuspStatus
                | CicsOutputName::BtsAbCode
                | CicsOutputName::BtsAbProgram
        ),
        CicsPlanOperation::RunTransId => matches!(output, CicsOutputName::BtsChildToken),
        _ => false,
    }
}

pub(super) fn invalid_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let operation = plan.operation;
    let allowed = allowed_inputs(operation);
    let required = required_inputs(operation);
    if inputs.iter().any(|name| !allowed.contains(name))
        || required.iter().any(|name| !inputs.contains(name))
        || plan
            .options
            .iter()
            .any(|option| !allowed_option(operation, *option))
        || required_selector(operation).is_some_and(|selector| !plan.options.contains(&selector))
        || operation == CicsPlanOperation::RunTransId
            && !outputs.contains(&CicsOutputName::BtsChildToken)
    {
        return true;
    }
    let running = matches!(
        operation,
        CicsPlanOperation::RunAcqActivity
            | CicsPlanOperation::RunAcqProcess
            | CicsPlanOperation::RunActivity
    );
    let sync = plan.options.contains(&CicsPlanOption::BtsSynchronous);
    let asynchronous = plan.options.contains(&CicsPlanOption::BtsAsynchronous);
    if running && sync == asynchronous
        || !running && (sync || asynchronous)
        || inputs.contains(&CicsOperandName::BtsFacilityToken) && !asynchronous
    {
        return true;
    }
    plan.operands.iter().any(|operand| {
        let maximum = match operand.name {
            CicsOperandName::BtsActivityId => 52,
            CicsOperandName::BtsProcess => 36,
            CicsOperandName::BtsProcessType
            | CicsOperandName::BtsProgram
            | CicsOperandName::BtsUserId
            | CicsOperandName::BtsFacilityToken => 8,
            CicsOperandName::BtsTransId => 4,
            CicsOperandName::BtsActivity
            | CicsOperandName::BtsEvent
            | CicsOperandName::BtsInputEvent
            | CicsOperandName::BtsChannel => 16,
            _ => return true,
        };
        match &operand.value {
            CicsOperandValue::Literal(bytes) => {
                let length = match operand.name {
                    CicsOperandName::BtsProcess
                    | CicsOperandName::BtsActivity
                    | CicsOperandName::BtsChannel => {
                        let Ok(text) = std::str::from_utf8(bytes) else {
                            return true;
                        };
                        text.chars().count()
                    }
                    _ => bytes.len(),
                };
                bytes.is_empty()
                    || length > maximum
                    || operand.name == CicsOperandName::BtsFacilityToken && bytes.len() != 8
            }
            CicsOperandValue::Storage(_) => false,
            CicsOperandValue::Integer(_) | CicsOperandValue::LengthOf(_) => true,
        }
    })
}

fn allowed_inputs(operation: CicsPlanOperation) -> &'static [CicsOperandName] {
    use CicsOperandName as O;
    match operation {
        CicsPlanOperation::AcquireActivityId => &[O::BtsActivityId],
        CicsPlanOperation::AcquireProcess => &[O::BtsProcess, O::BtsProcessType],
        CicsPlanOperation::CancelActivity
        | CicsPlanOperation::CheckActivity
        | CicsPlanOperation::DeleteActivity
        | CicsPlanOperation::ResetActivity
        | CicsPlanOperation::ResumeActivity
        | CicsPlanOperation::SuspendActivity => &[O::BtsActivity],
        CicsPlanOperation::DefineActivity => &[
            O::BtsActivity,
            O::BtsEvent,
            O::BtsTransId,
            O::BtsProgram,
            O::BtsUserId,
        ],
        CicsPlanOperation::DefineProcess => &[
            O::BtsProcess,
            O::BtsProcessType,
            O::BtsTransId,
            O::BtsProgram,
            O::BtsUserId,
        ],
        CicsPlanOperation::RunAcqActivity | CicsPlanOperation::RunAcqProcess => {
            &[O::BtsInputEvent, O::BtsFacilityToken]
        }
        CicsPlanOperation::RunActivity => &[O::BtsActivity, O::BtsInputEvent, O::BtsFacilityToken],
        CicsPlanOperation::RunTransId => &[O::BtsTransId, O::BtsChannel],
        _ => &[],
    }
}

fn required_inputs(operation: CicsPlanOperation) -> &'static [CicsOperandName] {
    use CicsOperandName as O;
    match operation {
        CicsPlanOperation::AcquireActivityId => &[O::BtsActivityId],
        CicsPlanOperation::AcquireProcess => &[O::BtsProcess, O::BtsProcessType],
        CicsPlanOperation::CancelActivity
        | CicsPlanOperation::CheckActivity
        | CicsPlanOperation::DeleteActivity
        | CicsPlanOperation::ResetActivity
        | CicsPlanOperation::ResumeActivity
        | CicsPlanOperation::RunActivity
        | CicsPlanOperation::SuspendActivity => &[O::BtsActivity],
        CicsPlanOperation::DefineActivity => &[O::BtsActivity, O::BtsTransId],
        CicsPlanOperation::DefineProcess => &[O::BtsProcess, O::BtsProcessType, O::BtsTransId],
        CicsPlanOperation::RunTransId => &[O::BtsTransId],
        _ => &[],
    }
}

fn required_selector(operation: CicsPlanOperation) -> Option<CicsPlanOption> {
    match operation {
        CicsPlanOperation::CancelAcqActivity
        | CicsPlanOperation::CheckAcqActivity
        | CicsPlanOperation::ResumeAcqActivity
        | CicsPlanOperation::RunAcqActivity
        | CicsPlanOperation::SuspendAcqActivity => Some(CicsPlanOption::AcqActivity),
        CicsPlanOperation::CancelAcqProcess
        | CicsPlanOperation::CheckAcqProcess
        | CicsPlanOperation::ResetAcqProcess
        | CicsPlanOperation::ResumeAcqProcess
        | CicsPlanOperation::RunAcqProcess
        | CicsPlanOperation::SuspendAcqProcess => Some(CicsPlanOption::AcqProcess),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::codec_tags::{
        operand_from_tag, operand_tag, operation_from_tag, operation_tag, option_from_tag,
        option_tag, output_from_tag, output_tag,
    };
    use super::super::{
        CicsCondition, CicsNamedOperand, CicsPlanLimits, decode_cics_effect_plan,
        encode_cics_effect_plan, encode_cics_effect_plan_version,
    };
    use super::*;

    #[test]
    fn all_assigned_operation_tags_roundtrip_without_aliases() {
        let operations = [
            CicsPlanOperation::AcquireActivityId,
            CicsPlanOperation::AcquireProcess,
            CicsPlanOperation::CancelAcqActivity,
            CicsPlanOperation::CancelAcqProcess,
            CicsPlanOperation::CancelActivity,
            CicsPlanOperation::CheckAcqActivity,
            CicsPlanOperation::CheckAcqProcess,
            CicsPlanOperation::CheckActivity,
            CicsPlanOperation::DefineActivity,
            CicsPlanOperation::DefineProcess,
            CicsPlanOperation::DeleteActivity,
            CicsPlanOperation::ResetAcqProcess,
            CicsPlanOperation::ResetActivity,
            CicsPlanOperation::ResumeAcqActivity,
            CicsPlanOperation::ResumeAcqProcess,
            CicsPlanOperation::ResumeActivity,
            CicsPlanOperation::RunAcqActivity,
            CicsPlanOperation::RunAcqProcess,
            CicsPlanOperation::RunActivity,
            CicsPlanOperation::RunTransId,
            CicsPlanOperation::SuspendAcqActivity,
            CicsPlanOperation::SuspendAcqProcess,
            CicsPlanOperation::SuspendActivity,
        ];
        for (offset, operation) in operations.into_iter().enumerate() {
            let tag = 165 + offset as u16;
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag).unwrap(), operation);
        }
        for tag in 704..=714 {
            assert_eq!(operand_tag(operand_from_tag(tag).unwrap()), tag);
        }
        for tag in 636..=637 {
            assert_eq!(option_tag(option_from_tag(tag).unwrap()), tag);
        }
        for tag in 760..=766 {
            assert_eq!(output_tag(output_from_tag(tag).unwrap()), tag);
        }
    }

    #[test]
    fn acquisition_plan_is_v2_canonical_and_v1_remains_rejected() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::AcquireProcess,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::BtsProcess,
                    value: CicsOperandValue::Literal(b"ORDER".to_vec()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::BtsProcessType,
                    value: CicsOperandValue::Literal(b"TYPE".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(&bytes[..6], b"MCEP\0\x02");
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);
        assert!(encode_cics_effect_plan_version(&plan, limits, 1).is_err());
    }

    #[test]
    fn source_character_widths_roundtrip_and_reject_malformed_literals() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::AcquireProcess,
            operands: vec![
                CicsNamedOperand {
                    name: CicsOperandName::BtsProcess,
                    value: CicsOperandValue::Literal(format!("{}¬", "P".repeat(35)).into_bytes()),
                },
                CicsNamedOperand {
                    name: CicsOperandName::BtsProcessType,
                    value: CicsOperandValue::Literal(b"TYPE".to_vec()),
                },
            ],
            options: BTreeSet::new(),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&plan, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits).unwrap(), plan);
        plan.operands[0].value =
            CicsOperandValue::Literal(format!("{}¬", "P".repeat(36)).into_bytes());
        assert!(encode_cics_effect_plan(&plan, limits).is_err());
        plan.operands[0].value = CicsOperandValue::Literal(vec![0xff]);
        assert!(encode_cics_effect_plan(&plan, limits).is_err());
    }

    #[test]
    fn selector_and_execution_mode_are_not_dropped() {
        let mut plan = CicsEffectPlan {
            operation: CicsPlanOperation::RunAcqProcess,
            operands: Vec::new(),
            options: BTreeSet::from([CicsPlanOption::AcqProcess, CicsPlanOption::BtsSynchronous]),
            outputs: Vec::new(),
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        assert!(encode_cics_effect_plan(&plan, limits).is_ok());
        plan.options.insert(CicsPlanOption::BtsAsynchronous);
        assert!(encode_cics_effect_plan(&plan, limits).is_err());
        plan.options.remove(&CicsPlanOption::BtsSynchronous);
        plan.options.remove(&CicsPlanOption::AcqProcess);
        assert!(encode_cics_effect_plan(&plan, limits).is_err());
    }
}
