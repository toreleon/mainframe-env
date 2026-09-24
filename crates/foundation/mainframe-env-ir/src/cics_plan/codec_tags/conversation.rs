use super::*;

pub(super) const fn operation_tag(value: CicsPlanOperation) -> u16 {
    match value {
        CicsPlanOperation::AllocateConversation => 222,
        CicsPlanOperation::GdsAllocateConversation => 223,
        CicsPlanOperation::GdsAssignConversation => 224,
        CicsPlanOperation::BuildAttach => 225,
        CicsPlanOperation::ConnectProcess => 226,
        CicsPlanOperation::GdsConnectProcess => 227,
        CicsPlanOperation::Converse => 228,
        CicsPlanOperation::FreeConversation => 229,
        CicsPlanOperation::GdsFreeConversation => 230,
        _ => panic!("unmapped conversation operation"),
    }
}

pub(super) fn operation_from_tag(value: u16) -> Result<CicsPlanOperation, CicsPlanCodecProblem> {
    match value {
        222 => Ok(CicsPlanOperation::AllocateConversation),
        223 => Ok(CicsPlanOperation::GdsAllocateConversation),
        224 => Ok(CicsPlanOperation::GdsAssignConversation),
        225 => Ok(CicsPlanOperation::BuildAttach),
        226 => Ok(CicsPlanOperation::ConnectProcess),
        227 => Ok(CicsPlanOperation::GdsConnectProcess),
        228 => Ok(CicsPlanOperation::Converse),
        229 => Ok(CicsPlanOperation::FreeConversation),
        230 => Ok(CicsPlanOperation::GdsFreeConversation),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}

pub(super) const fn operand_tag(value: CicsOperandName) -> u16 {
    match value {
        CicsOperandName::ConversationSysid => 1216,
        CicsOperandName::ConversationPartner => 1217,
        CicsOperandName::ConversationProfile => 1218,
        CicsOperandName::ConversationSession => 1219,
        CicsOperandName::ConversationModeName => 1220,
        CicsOperandName::ConversationConvid => 1221,
        CicsOperandName::ConversationAttachId => 1222,
        CicsOperandName::ConversationProcess => 1223,
        CicsOperandName::ConversationResource => 1224,
        CicsOperandName::ConversationReturnProcess => 1225,
        CicsOperandName::ConversationReturnResource => 1226,
        CicsOperandName::ConversationQueue => 1227,
        CicsOperandName::ConversationIuType => 1228,
        CicsOperandName::ConversationDataStream => 1229,
        CicsOperandName::ConversationRecordFormat => 1230,
        CicsOperandName::ConversationProcName => 1231,
        CicsOperandName::ConversationProcLength => 1232,
        CicsOperandName::ConversationPipList => 1233,
        CicsOperandName::ConversationPipLength => 1234,
        CicsOperandName::ConversationSyncLevel => 1235,
        CicsOperandName::ConversationFrom => 1236,
        CicsOperandName::ConversationFromLength => 1237,
        CicsOperandName::ConversationFromFullLength => 1238,
        CicsOperandName::ConversationMaxLength => 1239,
        CicsOperandName::ConversationMaxFullLength => 1240,
        CicsOperandName::ConversationToLength => 1241,
        CicsOperandName::ConversationToFullLength => 1242,
        _ => panic!("unmapped conversation operand"),
    }
}

pub(super) fn operand_from_tag(value: u16) -> Result<CicsOperandName, CicsPlanCodecProblem> {
    match value {
        1216 => Ok(CicsOperandName::ConversationSysid),
        1217 => Ok(CicsOperandName::ConversationPartner),
        1218 => Ok(CicsOperandName::ConversationProfile),
        1220 => Ok(CicsOperandName::ConversationModeName),
        1223 => Ok(CicsOperandName::ConversationProcess),
        1224 => Ok(CicsOperandName::ConversationResource),
        1225 => Ok(CicsOperandName::ConversationReturnProcess),
        1226 => Ok(CicsOperandName::ConversationReturnResource),
        1227 => Ok(CicsOperandName::ConversationQueue),
        1228 => Ok(CicsOperandName::ConversationIuType),
        1229 => Ok(CicsOperandName::ConversationDataStream),
        1230 => Ok(CicsOperandName::ConversationRecordFormat),
        1231 => Ok(CicsOperandName::ConversationProcName),
        1232 => Ok(CicsOperandName::ConversationProcLength),
        1233 => Ok(CicsOperandName::ConversationPipList),
        1234 => Ok(CicsOperandName::ConversationPipLength),
        1235 => Ok(CicsOperandName::ConversationSyncLevel),
        1236 => Ok(CicsOperandName::ConversationFrom),
        1237 => Ok(CicsOperandName::ConversationFromLength),
        1238 => Ok(CicsOperandName::ConversationFromFullLength),
        1239 => Ok(CicsOperandName::ConversationMaxLength),
        1240 => Ok(CicsOperandName::ConversationMaxFullLength),
        1241 => Ok(CicsOperandName::ConversationToLength),
        1242 => Ok(CicsOperandName::ConversationToFullLength),
        _ => Err(CicsPlanCodecProblem::Malformed),
    }
}
