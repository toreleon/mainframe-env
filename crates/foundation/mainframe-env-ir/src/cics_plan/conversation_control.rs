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
    let allowed = match plan.operation {
        CicsPlanOperation::AllocateConversation => &[
            CicsOperandName::ConversationSysid,
            CicsOperandName::ConversationPartner,
            CicsOperandName::ConversationProfile,
            CicsOperandName::ConversationSession,
        ][..],
        CicsPlanOperation::GdsAllocateConversation => &[
            CicsOperandName::ConversationSysid,
            CicsOperandName::ConversationPartner,
            CicsOperandName::ConversationModeName,
        ],
        CicsPlanOperation::GdsAssignConversation => &[],
        CicsPlanOperation::BuildAttach => &[
            CicsOperandName::ConversationAttachId,
            CicsOperandName::ConversationProcess,
            CicsOperandName::ConversationResource,
            CicsOperandName::ConversationReturnProcess,
            CicsOperandName::ConversationReturnResource,
            CicsOperandName::ConversationQueue,
            CicsOperandName::ConversationIuType,
            CicsOperandName::ConversationDataStream,
            CicsOperandName::ConversationRecordFormat,
        ],
        CicsPlanOperation::ConnectProcess | CicsPlanOperation::GdsConnectProcess => &[
            CicsOperandName::ConversationConvid,
            CicsOperandName::ConversationSession,
            CicsOperandName::ConversationProcName,
            CicsOperandName::ConversationProcLength,
            CicsOperandName::ConversationPartner,
            CicsOperandName::ConversationPipList,
            CicsOperandName::ConversationPipLength,
            CicsOperandName::ConversationSyncLevel,
        ],
        CicsPlanOperation::Converse => &[
            CicsOperandName::ConversationConvid,
            CicsOperandName::ConversationSession,
            CicsOperandName::ConversationAttachId,
            CicsOperandName::ConversationFrom,
            CicsOperandName::ConversationFromLength,
            CicsOperandName::ConversationFromFullLength,
            CicsOperandName::ConversationMaxLength,
            CicsOperandName::ConversationMaxFullLength,
            CicsOperandName::ConversationToLength,
            CicsOperandName::ConversationToFullLength,
        ],
        CicsPlanOperation::FreeConversation | CicsPlanOperation::GdsFreeConversation => &[
            CicsOperandName::ConversationConvid,
            CicsOperandName::ConversationSession,
        ],
        CicsPlanOperation::ReceiveConversation | CicsPlanOperation::GdsReceiveConversation => &[
            CicsOperandName::ConversationDataConvid,
            CicsOperandName::ConversationDataSession,
            CicsOperandName::ConversationDataLength,
            CicsOperandName::ConversationDataFullLength,
            CicsOperandName::ConversationDataMaxLength,
            CicsOperandName::ConversationDataMaxFullLength,
        ],
        CicsPlanOperation::SendConversation => &[
            CicsOperandName::ConversationDataConvid,
            CicsOperandName::ConversationDataSession,
            CicsOperandName::ConversationDataFrom,
            CicsOperandName::ConversationDataLength,
            CicsOperandName::ConversationDataFullLength,
            CicsOperandName::ConversationDataAttachId,
        ],
        CicsPlanOperation::GdsWaitConversation | CicsPlanOperation::WaitConvid => {
            &[CicsOperandName::ConversationDataConvid]
        }
        CicsPlanOperation::WaitSignal => &[],
        CicsPlanOperation::WaitTerminal => &[
            CicsOperandName::ConversationDataConvid,
            CicsOperandName::ConversationDataSession,
        ],
        _ => return true,
    };
    if !inputs.is_subset(&allowed.iter().copied().collect())
        || outputs
            .iter()
            .any(|name| !output_allowed(plan.operation, *name))
        || plan
            .options
            .iter()
            .any(|option| !option_allowed(plan.operation, *option))
    {
        return true;
    }
    let has = |name| inputs.contains(&name);
    let out = |name| outputs.contains(&name);
    let exactly_one = |left, right| has(left) != has(right);
    let process_pair = exactly_one(
        CicsOperandName::ConversationProcName,
        CicsOperandName::ConversationPartner,
    );
    let wrong = match plan.operation {
        CicsPlanOperation::AllocateConversation | CicsPlanOperation::GdsAllocateConversation => {
            !exactly_one(
                CicsOperandName::ConversationSysid,
                CicsOperandName::ConversationPartner,
            ) || plan.operation == CicsPlanOperation::GdsAllocateConversation
                && (!out(CicsOutputName::ConversationConvid)
                    || !out(CicsOutputName::ConversationRetcode))
        }
        CicsPlanOperation::GdsAssignConversation => !out(CicsOutputName::ConversationRetcode),
        CicsPlanOperation::BuildAttach => !has(CicsOperandName::ConversationAttachId),
        CicsPlanOperation::ConnectProcess | CicsPlanOperation::GdsConnectProcess => {
            !has(CicsOperandName::ConversationConvid) && !has(CicsOperandName::ConversationSession)
                || !process_pair
                || has(CicsOperandName::ConversationProcLength)
                    != has(CicsOperandName::ConversationProcName)
                || has(CicsOperandName::ConversationPipList)
                    != has(CicsOperandName::ConversationPipLength)
                || plan.operation == CicsPlanOperation::GdsConnectProcess
                    && !out(CicsOutputName::ConversationRetcode)
        }
        CicsPlanOperation::Converse => {
            !has(CicsOperandName::ConversationFrom) && !has(CicsOperandName::ConversationAttachId)
                || !out(CicsOutputName::ConversationInto) && !out(CicsOutputName::ConversationSet)
                || out(CicsOutputName::ConversationInto) && out(CicsOutputName::ConversationSet)
                || has(CicsOperandName::ConversationFromLength)
                    && has(CicsOperandName::ConversationFromFullLength)
                || has(CicsOperandName::ConversationMaxLength)
                    && has(CicsOperandName::ConversationMaxFullLength)
                || out(CicsOutputName::ConversationToLength)
                    && out(CicsOutputName::ConversationToFullLength)
        }
        CicsPlanOperation::FreeConversation => false,
        CicsPlanOperation::GdsFreeConversation => {
            !has(CicsOperandName::ConversationConvid) || !out(CicsOutputName::ConversationRetcode)
        }
        CicsPlanOperation::ReceiveConversation => {
            has(CicsOperandName::ConversationDataConvid)
                && has(CicsOperandName::ConversationDataSession)
                || !out(CicsOutputName::ConversationDataInto)
                    && !out(CicsOutputName::ConversationDataSet)
                || out(CicsOutputName::ConversationDataInto)
                    && out(CicsOutputName::ConversationDataSet)
                || has(CicsOperandName::ConversationDataLength)
                    && has(CicsOperandName::ConversationDataFullLength)
                || has(CicsOperandName::ConversationDataMaxLength)
                    && has(CicsOperandName::ConversationDataMaxFullLength)
                || out(CicsOutputName::ConversationDataLength)
                    && out(CicsOutputName::ConversationDataFullLength)
        }
        CicsPlanOperation::GdsReceiveConversation => {
            !has(CicsOperandName::ConversationDataConvid)
                || has(CicsOperandName::ConversationDataSession)
                || !has(CicsOperandName::ConversationDataMaxFullLength)
                || !out(CicsOutputName::ConversationDataRetcode)
                || !out(CicsOutputName::ConversationDataFullLength)
                || out(CicsOutputName::ConversationDataInto)
                    == out(CicsOutputName::ConversationDataSet)
        }
        CicsPlanOperation::SendConversation => {
            !has(CicsOperandName::ConversationDataFrom)
                || has(CicsOperandName::ConversationDataConvid)
                    && has(CicsOperandName::ConversationDataSession)
                || has(CicsOperandName::ConversationDataLength)
                    == has(CicsOperandName::ConversationDataFullLength)
        }
        CicsPlanOperation::GdsWaitConversation => !out(CicsOutputName::ConversationDataRetcode),
        CicsPlanOperation::WaitConvid => !has(CicsOperandName::ConversationDataConvid),
        CicsPlanOperation::WaitSignal => false,
        CicsPlanOperation::WaitTerminal => {
            has(CicsOperandName::ConversationDataConvid)
                && has(CicsOperandName::ConversationDataSession)
        }
        _ => true,
    };
    wrong
        || plan.operands.iter().any(|operand| {
            let numeric = matches!(
                operand.name,
                CicsOperandName::ConversationIuType
                    | CicsOperandName::ConversationDataStream
                    | CicsOperandName::ConversationRecordFormat
                    | CicsOperandName::ConversationProcLength
                    | CicsOperandName::ConversationPipLength
                    | CicsOperandName::ConversationSyncLevel
                    | CicsOperandName::ConversationFromLength
                    | CicsOperandName::ConversationFromFullLength
                    | CicsOperandName::ConversationMaxLength
                    | CicsOperandName::ConversationMaxFullLength
                    | CicsOperandName::ConversationToLength
                    | CicsOperandName::ConversationToFullLength
                    | CicsOperandName::ConversationDataLength
                    | CicsOperandName::ConversationDataFullLength
                    | CicsOperandName::ConversationDataMaxLength
                    | CicsOperandName::ConversationDataMaxFullLength
            );
            if numeric {
                !matches!(
                    operand.value,
                    CicsOperandValue::Integer(_) | CicsOperandValue::Storage(_)
                )
            } else {
                !matches!(
                    operand.value,
                    CicsOperandValue::Literal(_) | CicsOperandValue::Storage(_)
                )
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageId;
    use crate::cics_plan::{
        CicsCondition, CicsNamedOperand, CicsOutputBinding, CicsPlanCodecProblem, CicsPlanLimits,
        CicsStorageSlot, LEGACY_VERSION, codec_tags::*, decode_cics_effect_plan,
        encode_cics_effect_plan, encode_cics_effect_plan_version,
    };

    fn slot(index: usize) -> CicsStorageSlot {
        CicsStorageSlot {
            storage: StorageId::from_index(index).unwrap(),
            qualified_layout_name: format!("CONVERSATION.FIELD-{index}"),
        }
    }

    #[test]
    fn reserved_tag_ranges_are_exact_and_unique() {
        let operations = [
            CicsPlanOperation::AllocateConversation,
            CicsPlanOperation::GdsAllocateConversation,
            CicsPlanOperation::GdsAssignConversation,
            CicsPlanOperation::BuildAttach,
            CicsPlanOperation::ConnectProcess,
            CicsPlanOperation::GdsConnectProcess,
            CicsPlanOperation::Converse,
            CicsPlanOperation::FreeConversation,
            CicsPlanOperation::GdsFreeConversation,
        ];
        for (index, operation) in operations.into_iter().enumerate() {
            let tag = 222 + index as u16;
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        assert_eq!(operand_tag(CicsOperandName::ConversationSysid), 1216);
        assert_eq!(operand_tag(CicsOperandName::ConversationToFullLength), 1242);
        assert_eq!(
            operand_from_tag(1242),
            Ok(CicsOperandName::ConversationToFullLength)
        );
        assert_eq!(option_tag(CicsPlanOption::ConversationNoQueue), 1148);
        assert_eq!(option_tag(CicsPlanOption::ConversationFmh), 1151);
        assert_eq!(option_from_tag(1151), Ok(CicsPlanOption::ConversationFmh));
        assert_eq!(output_tag(CicsOutputName::ConversationState), 1272);
        assert_eq!(output_tag(CicsOutputName::ConversationToFullLength), 1281);
        assert_eq!(
            output_from_tag(1281),
            Ok(CicsOutputName::ConversationToFullLength)
        );
        assert_eq!(operand_from_tag(1344), Err(CicsPlanCodecProblem::Malformed));
        assert_eq!(option_from_tag(1276), Err(CicsPlanCodecProblem::Malformed));
        assert_eq!(output_from_tag(1400), Err(CicsPlanCodecProblem::Malformed));
    }

    #[test]
    fn mapped_allocate_roundtrips_v2_and_rejects_v1() {
        let plan = CicsEffectPlan {
            operation: CicsPlanOperation::AllocateConversation,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::ConversationSysid,
                value: CicsOperandValue::Literal(b"SYS1".to_vec()),
            }],
            options: BTreeSet::from([CicsPlanOption::ConversationNoQueue]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::ConversationState,
                target: slot(0),
            }],
            condition: CicsCondition::Default,
        };
        let encoded = encode_cics_effect_plan(&plan, CicsPlanLimits::default()).unwrap();
        assert_eq!(
            decode_cics_effect_plan(&encoded, CicsPlanLimits::default()),
            Ok(plan.clone())
        );
        assert_eq!(
            encode_cics_effect_plan_version(&plan, CicsPlanLimits::default(), LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        let mut invalid = plan;
        invalid.operands.push(CicsNamedOperand {
            name: CicsOperandName::ConversationFrom,
            value: CicsOperandValue::Literal(b"BAD".to_vec()),
        });
        assert_eq!(
            encode_cics_effect_plan(&invalid, CicsPlanLimits::default()),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }

    #[test]
    fn data_wait_tags_and_source_shapes_are_v2_only() {
        let operations = [
            CicsPlanOperation::ReceiveConversation,
            CicsPlanOperation::GdsReceiveConversation,
            CicsPlanOperation::SendConversation,
            CicsPlanOperation::GdsWaitConversation,
            CicsPlanOperation::WaitConvid,
            CicsPlanOperation::WaitSignal,
            CicsPlanOperation::WaitTerminal,
        ];
        for (index, operation) in operations.into_iter().enumerate() {
            let tag = 259 + index as u16;
            assert_eq!(operation_tag(operation), tag);
            assert_eq!(operation_from_tag(tag), Ok(operation));
        }
        assert_eq!(operand_tag(CicsOperandName::ConversationDataConvid), 1600);
        assert_eq!(operand_tag(CicsOperandName::ConversationDataAttachId), 1607);
        assert_eq!(option_tag(CicsPlanOption::ConversationDataNotruncate), 1532);
        assert_eq!(option_tag(CicsPlanOption::ConversationDataDefresp), 1540);
        assert_eq!(output_tag(CicsOutputName::ConversationDataInto), 1656);
        assert_eq!(output_tag(CicsOutputName::ConversationDataState), 1662);

        let mut receive = CicsEffectPlan {
            operation: CicsPlanOperation::ReceiveConversation,
            operands: vec![CicsNamedOperand {
                name: CicsOperandName::ConversationDataConvid,
                value: CicsOperandValue::Literal(b"ABCD".to_vec()),
            }],
            options: BTreeSet::from([CicsPlanOption::ConversationDataNotruncate]),
            outputs: vec![CicsOutputBinding {
                name: CicsOutputName::ConversationDataInto,
                target: slot(1),
            }],
            condition: CicsCondition::Default,
        };
        let limits = CicsPlanLimits::default();
        let bytes = encode_cics_effect_plan(&receive, limits).unwrap();
        assert_eq!(decode_cics_effect_plan(&bytes, limits), Ok(receive.clone()));
        assert_eq!(
            encode_cics_effect_plan_version(&receive, limits, LEGACY_VERSION),
            Err(CicsPlanCodecProblem::Malformed)
        );
        receive
            .options
            .insert(CicsPlanOption::ConversationDataInvite);
        assert_eq!(
            encode_cics_effect_plan(&receive, limits),
            Err(CicsPlanCodecProblem::Malformed)
        );
    }
}

pub(super) fn option_allowed(operation: CicsPlanOperation, option: CicsPlanOption) -> bool {
    if option == CicsPlanOption::NoHandle {
        return true;
    }
    matches!(
        (operation, option),
        (
            CicsPlanOperation::ReceiveConversation,
            CicsPlanOption::ConversationDataNotruncate
        ) | (
            CicsPlanOperation::GdsReceiveConversation,
            CicsPlanOption::ConversationDataBuffer | CicsPlanOption::ConversationDataLlid
        ) | (
            CicsPlanOperation::SendConversation,
            CicsPlanOption::ConversationDataInvite
                | CicsPlanOption::ConversationDataLast
                | CicsPlanOption::ConversationDataConfirm
                | CicsPlanOption::ConversationDataWait
                | CicsPlanOption::ConversationDataFmh
                | CicsPlanOption::ConversationDataDefresp
        ) | (
            CicsPlanOperation::AllocateConversation | CicsPlanOperation::GdsAllocateConversation,
            CicsPlanOption::ConversationNoQueue
        ) | (
            CicsPlanOperation::Converse,
            CicsPlanOption::ConversationNotruncate
                | CicsPlanOption::ConversationDefresp
                | CicsPlanOption::ConversationFmh
        )
    )
}

pub(super) const fn output_allowed(operation: CicsPlanOperation, output: CicsOutputName) -> bool {
    if matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2) {
        return true;
    }
    matches!(
        (operation, output),
        (
            CicsPlanOperation::ReceiveConversation
                | CicsPlanOperation::GdsReceiveConversation
                | CicsPlanOperation::SendConversation
                | CicsPlanOperation::GdsWaitConversation
                | CicsPlanOperation::WaitConvid,
            CicsOutputName::ConversationDataState
        ) | (
            CicsPlanOperation::ReceiveConversation | CicsPlanOperation::GdsReceiveConversation,
            CicsOutputName::ConversationDataInto
                | CicsOutputName::ConversationDataSet
                | CicsOutputName::ConversationDataLength
                | CicsOutputName::ConversationDataFullLength
        ) | (
            CicsPlanOperation::GdsReceiveConversation | CicsPlanOperation::GdsWaitConversation,
            CicsOutputName::ConversationDataRetcode | CicsOutputName::ConversationDataConvData
        ) | (
            CicsPlanOperation::AllocateConversation
                | CicsPlanOperation::GdsAllocateConversation
                | CicsPlanOperation::ConnectProcess
                | CicsPlanOperation::GdsConnectProcess
                | CicsPlanOperation::Converse
                | CicsPlanOperation::FreeConversation
                | CicsPlanOperation::GdsFreeConversation,
            CicsOutputName::ConversationState
        ) | (
            CicsPlanOperation::GdsAllocateConversation,
            CicsOutputName::ConversationConvid | CicsOutputName::ConversationRetcode
        ) | (
            CicsPlanOperation::GdsAssignConversation,
            CicsOutputName::ConversationPrinConvid
                | CicsOutputName::ConversationPrinSysid
                | CicsOutputName::ConversationRetcode
        ) | (
            CicsPlanOperation::GdsConnectProcess | CicsPlanOperation::GdsFreeConversation,
            CicsOutputName::ConversationRetcode | CicsOutputName::ConversationConvData
        ) | (
            CicsPlanOperation::Converse,
            CicsOutputName::ConversationInto
                | CicsOutputName::ConversationSet
                | CicsOutputName::ConversationToLength
                | CicsOutputName::ConversationToFullLength
        )
    )
}
