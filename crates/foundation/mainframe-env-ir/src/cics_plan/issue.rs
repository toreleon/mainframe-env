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
    if matches!(
        plan.operation,
        CicsPlanOperation::IssueCopy
            | CicsPlanOperation::IssueDisconnect
            | CicsPlanOperation::IssueEndfile
            | CicsPlanOperation::IssueEndoutput
            | CicsPlanOperation::IssueEods
            | CicsPlanOperation::IssueEraseAup
            | CicsPlanOperation::IssueLoad
            | CicsPlanOperation::IssuePass
            | CicsPlanOperation::IssuePrint
            | CicsPlanOperation::IssueReset
    ) {
        return invalid_device_shape(plan, inputs, outputs);
    }
    let basic = matches!(
        plan.operation,
        CicsPlanOperation::GdsIssueAbend
            | CicsPlanOperation::GdsIssueConfirmation
            | CicsPlanOperation::GdsIssueError
            | CicsPlanOperation::GdsIssuePrepare
            | CicsPlanOperation::GdsIssueSignal
    );
    let allowed_inputs = if basic {
        BTreeSet::from([CicsOperandName::IssueConvid])
    } else {
        BTreeSet::from([CicsOperandName::IssueConvid, CicsOperandName::IssueSession])
    };
    let allowed_outputs = if basic {
        BTreeSet::from([
            CicsOutputName::IssueState,
            CicsOutputName::IssueConvData,
            CicsOutputName::IssueRetCode,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ])
    } else {
        BTreeSet::from([
            CicsOutputName::IssueState,
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ])
    };
    !inputs.is_subset(&allowed_inputs)
        || !outputs.is_subset(&allowed_outputs)
        || !plan
            .options
            .is_subset(&BTreeSet::from([CicsPlanOption::NoHandle]))
        || inputs.contains(&CicsOperandName::IssueConvid)
            && inputs.contains(&CicsOperandName::IssueSession)
        || basic
            && (!inputs.contains(&CicsOperandName::IssueConvid)
                || !outputs.contains(&CicsOutputName::IssueConvData)
                || !outputs.contains(&CicsOutputName::IssueRetCode))
        || plan.operands.iter().any(|operand| {
            !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            )
        })
}

fn invalid_device_shape(
    plan: &CicsEffectPlan,
    inputs: &BTreeSet<CicsOperandName>,
    outputs: &BTreeSet<CicsOutputName>,
) -> bool {
    let (allowed_inputs, allowed_options, required_inputs) = match plan.operation {
        CicsPlanOperation::IssueCopy => (
            BTreeSet::from([CicsOperandName::IssueTermId, CicsOperandName::IssueCtlChar]),
            BTreeSet::from([CicsPlanOption::IssueWaitOption]),
            BTreeSet::from([CicsOperandName::IssueTermId]),
        ),
        CicsPlanOperation::IssueDisconnect => (
            BTreeSet::from([CicsOperandName::IssueSession]),
            BTreeSet::new(),
            BTreeSet::new(),
        ),
        CicsPlanOperation::IssueEndfile => (
            BTreeSet::new(),
            BTreeSet::from([CicsPlanOption::IssueEndOutput]),
            BTreeSet::new(),
        ),
        CicsPlanOperation::IssueEndoutput => (
            BTreeSet::new(),
            BTreeSet::from([CicsPlanOption::IssueEndFile]),
            BTreeSet::new(),
        ),
        CicsPlanOperation::IssueEraseAup => (
            BTreeSet::new(),
            BTreeSet::from([CicsPlanOption::IssueWaitOption]),
            BTreeSet::new(),
        ),
        CicsPlanOperation::IssueLoad => (
            BTreeSet::from([CicsOperandName::IssueProgram]),
            BTreeSet::from([CicsPlanOption::IssueConverse]),
            BTreeSet::from([CicsOperandName::IssueProgram]),
        ),
        CicsPlanOperation::IssuePass => (
            BTreeSet::from([
                CicsOperandName::IssueLuName,
                CicsOperandName::IssueFrom,
                CicsOperandName::IssueLength,
                CicsOperandName::IssueLogMode,
            ]),
            BTreeSet::from([
                CicsPlanOption::IssueLogonLogmode,
                CicsPlanOption::IssueNoQuiesce,
            ]),
            BTreeSet::from([CicsOperandName::IssueLuName]),
        ),
        CicsPlanOperation::IssueEods
        | CicsPlanOperation::IssuePrint
        | CicsPlanOperation::IssueReset => (BTreeSet::new(), BTreeSet::new(), BTreeSet::new()),
        _ => return true,
    };
    let mut allowed_options = allowed_options;
    allowed_options.insert(CicsPlanOption::NoHandle);
    !inputs.is_subset(&allowed_inputs)
        || !required_inputs.is_subset(inputs)
        || !outputs.is_subset(&BTreeSet::from([
            CicsOutputName::Resp,
            CicsOutputName::Resp2,
        ]))
        || !plan.options.is_subset(&allowed_options)
        || plan.operation == CicsPlanOperation::IssuePass
            && (inputs.contains(&CicsOperandName::IssueFrom)
                != inputs.contains(&CicsOperandName::IssueLength)
                || inputs.contains(&CicsOperandName::IssueLogMode)
                    && plan.options.contains(&CicsPlanOption::IssueLogonLogmode))
        || plan.operands.iter().any(|operand| match operand.name {
            CicsOperandName::IssueLength => !matches!(
                operand.value,
                CicsOperandValue::Integer(_)
                    | CicsOperandValue::Storage(_)
                    | CicsOperandValue::LengthOf(_)
            ),
            CicsOperandName::IssueCtlChar | CicsOperandName::IssueFrom => {
                !matches!(operand.value, CicsOperandValue::Storage(_))
            }
            _ => !matches!(
                operand.value,
                CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CicsCondition, CicsNamedOperand, CicsStorageSlot, StorageId};

    fn plan(
        operation: CicsPlanOperation,
        operands: Vec<CicsNamedOperand>,
        options: BTreeSet<CicsPlanOption>,
    ) -> CicsEffectPlan {
        CicsEffectPlan {
            operation,
            operands,
            options,
            outputs: vec![],
            condition: CicsCondition::Default,
        }
    }

    fn literal(name: CicsOperandName, bytes: &[u8]) -> CicsNamedOperand {
        CicsNamedOperand {
            name,
            value: CicsOperandValue::Literal(bytes.to_vec()),
        }
    }

    #[test]
    fn device_issue_source_options_reject_wrong_command_and_incomplete_pass_data() {
        let endfile = plan(
            CicsPlanOperation::IssueEndfile,
            vec![],
            BTreeSet::from([CicsPlanOption::IssueEndOutput]),
        );
        assert!(!invalid_shape(&endfile, &BTreeSet::new(), &BTreeSet::new()));
        let wrong_option = plan(
            CicsPlanOperation::IssueEndfile,
            vec![],
            BTreeSet::from([CicsPlanOption::IssueEndFile]),
        );
        assert!(invalid_shape(
            &wrong_option,
            &BTreeSet::new(),
            &BTreeSet::new()
        ));

        let from = CicsNamedOperand {
            name: CicsOperandName::IssueFrom,
            value: CicsOperandValue::Storage(CicsStorageSlot {
                storage: StorageId::from_index(1).unwrap(),
                qualified_layout_name: "PASS.DATA".into(),
            }),
        };
        let pass = plan(
            CicsPlanOperation::IssuePass,
            vec![literal(CicsOperandName::IssueLuName, b"APPL1"), from],
            BTreeSet::new(),
        );
        assert!(invalid_shape(
            &pass,
            &BTreeSet::from([CicsOperandName::IssueLuName, CicsOperandName::IssueFrom]),
            &BTreeSet::new()
        ));

        let copy = plan(CicsPlanOperation::IssueCopy, vec![], BTreeSet::new());
        assert!(invalid_shape(&copy, &BTreeSet::new(), &BTreeSet::new()));
    }
}
