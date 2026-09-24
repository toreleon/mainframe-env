use super::{
    HirCicsConditionPolicy, HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation,
    HirCicsOption, HirCicsOutputBinding, HirCicsOutputName, HirCicsStatement, HirCicsValue,
    HirDataReference, HirProblem, HirResolvedStatement, HirStatement, StatementKind,
};
use mainframe_env_ir::{
    Attribute, BlockId, CICS_EXECUTABLE_DESCRIPTORS, CicsCondition, CicsEffectPlan,
    CicsNamedOperand, CicsOperandName, CicsOperandValue, CicsOperationContract, CicsOutputBinding,
    CicsOutputName, CicsPlanLimits, CicsPlanOperation, CicsPlanOption, CicsStorageSlot, Effect,
    ModuleBuilder, OperationCatalog, OperationIdentity, OperationSchema, OperationSemanticContract,
    StorageId, StorageReference, cics_executable_descriptor, cobol_layout_definition_identity,
    encode_cics_effect_plan,
};
use std::collections::BTreeMap;

pub(crate) const CICS_PLAN_ATTRIBUTE: &str = "cics_plan";

pub(crate) struct EncodedCicsEffect {
    pub bytes: Vec<u8>,
    pub storage: Vec<StorageReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CicsPlanProblem {
    MissingStorage(String),
    InvalidStorageExtent(String),
    InvalidPlan,
}

pub(crate) fn executable_identity(operation: HirCicsOperation) -> OperationIdentity {
    cics_executable_descriptor(plan_operation(operation)).identity()
}

pub(crate) fn operation_effects(operation: HirCicsOperation) -> Vec<Effect> {
    cics_executable_descriptor(plan_operation(operation))
        .effects
        .to_vec()
}

pub(crate) fn register_hir_operation(catalog: &mut OperationCatalog) {
    let identity = OperationIdentity::new("cobol.hir", StatementKind::ExecCics.slug(), 2)
        .expect("static typed CICS HIR identity");
    let mut schema = OperationSchema::pure(identity, 0, 0);
    schema.required_attributes = [CICS_PLAN_ATTRIBUTE.into()].into_iter().collect();
    schema.allowed_effects = CICS_EXECUTABLE_DESCRIPTORS
        .iter()
        .flat_map(|descriptor| descriptor.effects.iter().copied())
        .collect();
    schema.semantic_contract = OperationSemanticContract::CicsEffect(CicsOperationContract {
        plan_attribute: CICS_PLAN_ATTRIBUTE.into(),
        expected_operation: None,
        layout_definition_operation: None,
    });
    catalog
        .register(schema)
        .expect("unique typed CICS HIR operation");
}

pub(crate) fn register_executable_operations(catalog: &mut OperationCatalog) {
    for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
        let mut schema = OperationSchema::pure(descriptor.identity(), 0, 0);
        schema.required_attributes = [CICS_PLAN_ATTRIBUTE.into()].into_iter().collect();
        schema.allowed_effects = descriptor.effects.iter().copied().collect();
        schema.runtime_import = Some(descriptor.runtime_import.into());
        schema.semantic_contract = OperationSemanticContract::CicsEffect(CicsOperationContract {
            plan_attribute: CICS_PLAN_ATTRIBUTE.into(),
            expected_operation: Some(descriptor.operation),
            layout_definition_operation: Some(cobol_layout_definition_identity()),
        });
        catalog
            .register(schema)
            .expect("unique typed CICS executable operation");
    }
}

pub(crate) fn emit_hir_operation(
    statement: &HirStatement,
    builder: &mut ModuleBuilder,
    block: BlockId,
    storage_ids: &BTreeMap<String, StorageId>,
) -> Result<bool, HirProblem> {
    let Some(HirResolvedStatement::Cics(command)) = statement.resolved.as_ref() else {
        return Ok(false);
    };
    let encoded = encode_statement(command, storage_ids).map_err(|problem| {
        HirProblem::InvalidResolvedStatement(
            statement.kind,
            statement.line,
            format!("CICS plan construction failed: {problem:?}"),
        )
    })?;
    builder
        .add_operation(
            block,
            OperationIdentity::new("cobol.hir", StatementKind::ExecCics.slug(), 2)
                .map_err(|_| HirProblem::UnknownStatement(statement.line))?,
            Vec::new(),
            0,
            BTreeMap::from([
                ("line".into(), Attribute::Integer(statement.line as i64)),
                (CICS_PLAN_ATTRIBUTE.into(), Attribute::Bytes(encoded.bytes)),
            ]),
            operation_effects(command.operation),
            encoded.storage,
            statement.location.clone(),
        )
        .map_err(|_| HirProblem::StatementLimitExceeded)?;
    Ok(true)
}

pub(crate) fn encode_statement(
    command: &HirCicsStatement,
    storage_ids: &BTreeMap<String, StorageId>,
) -> Result<EncodedCicsEffect, CicsPlanProblem> {
    let mut context = PlanContext {
        storage_ids,
        references: BTreeMap::new(),
    };
    let operands = command
        .operands
        .iter()
        .map(|operand| context.operand(operand))
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = command
        .outputs
        .iter()
        .map(|output| context.output(output))
        .collect::<Result<Vec<_>, _>>()?;
    let condition = match &command.condition_policy {
        HirCicsConditionPolicy::Default => CicsCondition::Default,
        HirCicsConditionPolicy::NoHandle => CicsCondition::NoHandle,
        HirCicsConditionPolicy::Respond {
            response,
            response2,
        } => CicsCondition::Respond {
            response: context.slot(response)?,
            response2: response2
                .as_ref()
                .map(|response| context.slot(response))
                .transpose()?,
        },
    };
    let plan = CicsEffectPlan {
        operation: plan_operation(command.operation),
        operands,
        options: command
            .options
            .iter()
            .copied()
            .map(|option| {
                if command.operation == HirCicsOperation::DocumentRetrieve
                    && option == HirCicsOption::DataOnly
                {
                    CicsPlanOption::DocumentDataOnly
                } else {
                    plan_option(option)
                }
            })
            .collect(),
        outputs,
        condition,
    };
    let bytes = encode_cics_effect_plan(&plan, CicsPlanLimits::default())
        .map_err(|_| CicsPlanProblem::InvalidPlan)?;
    Ok(EncodedCicsEffect {
        bytes,
        storage: context.references.into_values().collect(),
    })
}

struct PlanContext<'a> {
    storage_ids: &'a BTreeMap<String, StorageId>,
    references: BTreeMap<StorageId, StorageReference>,
}

impl PlanContext<'_> {
    fn operand(
        &mut self,
        operand: &HirCicsNamedOperand,
    ) -> Result<CicsNamedOperand, CicsPlanProblem> {
        Ok(CicsNamedOperand {
            name: match operand.name {
                HirCicsOperandName::BtsChild => CicsOperandName::BtsChild,
                HirCicsOperandName::BtsActivity => CicsOperandName::BtsActivity,
                HirCicsOperandName::BtsInputEvent => CicsOperandName::BtsInputEvent,
                HirCicsOperandName::BtsTimeout => CicsOperandName::BtsTimeout,
                HirCicsOperandName::ConversationAttachId => CicsOperandName::ConversationAttachId,
                HirCicsOperandName::ConversationConvid => CicsOperandName::ConversationConvid,
                HirCicsOperandName::ConversationSession => CicsOperandName::ConversationSession,
                HirCicsOperandName::ConversationMaxProcLen => {
                    CicsOperandName::ConversationMaxProcLen
                }
                HirCicsOperandName::ConversationNetName => CicsOperandName::ConversationNetName,
                HirCicsOperandName::ConversationSysid => CicsOperandName::ConversationSysid,
                HirCicsOperandName::ConversationPartner => CicsOperandName::ConversationPartner,
                HirCicsOperandName::ConversationProfile => CicsOperandName::ConversationProfile,
                HirCicsOperandName::ConversationModeName => CicsOperandName::ConversationModeName,
                HirCicsOperandName::ConversationProcess => CicsOperandName::ConversationProcess,
                HirCicsOperandName::ConversationResource => CicsOperandName::ConversationResource,
                HirCicsOperandName::ConversationReturnProcess => {
                    CicsOperandName::ConversationReturnProcess
                }
                HirCicsOperandName::ConversationReturnResource => {
                    CicsOperandName::ConversationReturnResource
                }
                HirCicsOperandName::ConversationQueue => CicsOperandName::ConversationQueue,
                HirCicsOperandName::ConversationIuType => CicsOperandName::ConversationIuType,
                HirCicsOperandName::ConversationDataStream => {
                    CicsOperandName::ConversationDataStream
                }
                HirCicsOperandName::ConversationRecordFormat => {
                    CicsOperandName::ConversationRecordFormat
                }
                HirCicsOperandName::ConversationProcName => CicsOperandName::ConversationProcName,
                HirCicsOperandName::ConversationProcLength => {
                    CicsOperandName::ConversationProcLength
                }
                HirCicsOperandName::ConversationPipList => CicsOperandName::ConversationPipList,
                HirCicsOperandName::ConversationPipLength => CicsOperandName::ConversationPipLength,
                HirCicsOperandName::ConversationSyncLevel => CicsOperandName::ConversationSyncLevel,
                HirCicsOperandName::ConversationFrom => CicsOperandName::ConversationFrom,
                HirCicsOperandName::ConversationFromLength => {
                    CicsOperandName::ConversationFromLength
                }
                HirCicsOperandName::ConversationFromFullLength => {
                    CicsOperandName::ConversationFromFullLength
                }
                HirCicsOperandName::ConversationMaxLength => CicsOperandName::ConversationMaxLength,
                HirCicsOperandName::ConversationMaxFullLength => {
                    CicsOperandName::ConversationMaxFullLength
                }
                HirCicsOperandName::ConversationToLength => CicsOperandName::ConversationToLength,
                HirCicsOperandName::ConversationToFullLength => {
                    CicsOperandName::ConversationToFullLength
                }
                HirCicsOperandName::Abcode => CicsOperandName::Abcode,
                HirCicsOperandName::ResClass => CicsOperandName::ResClass,
                HirCicsOperandName::IssueConvid => CicsOperandName::IssueConvid,
                HirCicsOperandName::IssueSession => CicsOperandName::IssueSession,
                HirCicsOperandName::IssueTermId => CicsOperandName::IssueTermId,
                HirCicsOperandName::IssueCtlChar => CicsOperandName::IssueCtlChar,
                HirCicsOperandName::IssueProgram => CicsOperandName::IssueProgram,
                HirCicsOperandName::IssueLuName => CicsOperandName::IssueLuName,
                HirCicsOperandName::IssueFrom => CicsOperandName::IssueFrom,
                HirCicsOperandName::IssueLength => CicsOperandName::IssueLength,
                HirCicsOperandName::IssueLogMode => CicsOperandName::IssueLogMode,
                HirCicsOperandName::ResId => CicsOperandName::ResId,
                HirCicsOperandName::ResIdLength => CicsOperandName::ResIdLength,
                HirCicsOperandName::ResType => CicsOperandName::ResType,
                HirCicsOperandName::LogMessage => CicsOperandName::LogMessage,
                HirCicsOperandName::SecurityUserId => CicsOperandName::SecurityUserId,
                HirCicsOperandName::SecurityGroupId => CicsOperandName::SecurityGroupId,
                HirCicsOperandName::SecurityPassword => CicsOperandName::SecurityPassword,
                HirCicsOperandName::SecurityNewPassword => CicsOperandName::SecurityNewPassword,
                HirCicsOperandName::SecurityNewPhrase => CicsOperandName::SecurityNewPhrase,
                HirCicsOperandName::SecurityNewPhraseLen => CicsOperandName::SecurityNewPhraseLen,
                HirCicsOperandName::SecurityEsmAppName => CicsOperandName::SecurityEsmAppName,
                HirCicsOperandName::SecurityTokenData => CicsOperandName::SecurityTokenData,
                HirCicsOperandName::SecurityTokenLength => CicsOperandName::SecurityTokenLength,
                HirCicsOperandName::SecurityEncryptKey => CicsOperandName::SecurityEncryptKey,
                HirCicsOperandName::SecurityLanguageCode => CicsOperandName::SecurityLanguageCode,
                HirCicsOperandName::SecurityNatLang => CicsOperandName::SecurityNatLang,
                HirCicsOperandName::SecurityOidCard => CicsOperandName::SecurityOidCard,
                HirCicsOperandName::SecurityPhrase => CicsOperandName::SecurityPhrase,
                HirCicsOperandName::SecurityPhraseLen => CicsOperandName::SecurityPhraseLen,
                HirCicsOperandName::Event => CicsOperandName::Event,
                HirCicsOperandName::SubEvent => CicsOperandName::SubEvent,
                HirCicsOperandName::SubEvent1 => CicsOperandName::SubEvent1,
                HirCicsOperandName::SubEvent2 => CicsOperandName::SubEvent2,
                HirCicsOperandName::SubEvent3 => CicsOperandName::SubEvent3,
                HirCicsOperandName::SubEvent4 => CicsOperandName::SubEvent4,
                HirCicsOperandName::SubEvent5 => CicsOperandName::SubEvent5,
                HirCicsOperandName::SubEvent6 => CicsOperandName::SubEvent6,
                HirCicsOperandName::SubEvent7 => CicsOperandName::SubEvent7,
                HirCicsOperandName::SubEvent8 => CicsOperandName::SubEvent8,
                HirCicsOperandName::SignalFrom => CicsOperandName::SignalFrom,
                HirCicsOperandName::SignalFromLength => CicsOperandName::SignalFromLength,
                HirCicsOperandName::SignalFromChannel => CicsOperandName::SignalFromChannel,
                HirCicsOperandName::Timer => CicsOperandName::Timer,
                HirCicsOperandName::TimerDays => CicsOperandName::TimerDays,
                HirCicsOperandName::TimerHours => CicsOperandName::TimerHours,
                HirCicsOperandName::TimerMinutes => CicsOperandName::TimerMinutes,
                HirCicsOperandName::TimerSeconds => CicsOperandName::TimerSeconds,
                HirCicsOperandName::TimerYear => CicsOperandName::TimerYear,
                HirCicsOperandName::TimerMonth => CicsOperandName::TimerMonth,
                HirCicsOperandName::TimerDayOfMonth => CicsOperandName::TimerDayOfMonth,
                HirCicsOperandName::TimerDayOfYear => CicsOperandName::TimerDayOfYear,
                HirCicsOperandName::Label => CicsOperandName::Label,
                HirCicsOperandName::Program => CicsOperandName::Program,
                HirCicsOperandName::Commarea => CicsOperandName::Commarea,
                HirCicsOperandName::TransId => CicsOperandName::TransId,
                HirCicsOperandName::BrExit => CicsOperandName::BrExit,
                HirCicsOperandName::BrData => CicsOperandName::BrData,
                HirCicsOperandName::BrDataLength => CicsOperandName::BrDataLength,
                HirCicsOperandName::TermId => CicsOperandName::TermId,
                HirCicsOperandName::ReturnTransId => CicsOperandName::ReturnTransId,
                HirCicsOperandName::ReturnTermId => CicsOperandName::ReturnTermId,
                HirCicsOperandName::UserId => CicsOperandName::UserId,
                HirCicsOperandName::File => CicsOperandName::File,
                HirCicsOperandName::Dataset => CicsOperandName::Dataset,
                HirCicsOperandName::From => CicsOperandName::From,
                HirCicsOperandName::Ridfld => CicsOperandName::Ridfld,
                HirCicsOperandName::Token => CicsOperandName::Token,
                HirCicsOperandName::Queue => CicsOperandName::Queue,
                HirCicsOperandName::Qname => CicsOperandName::Qname,
                HirCicsOperandName::SysId => CicsOperandName::SysId,
                HirCicsOperandName::Item => CicsOperandName::Item,
                HirCicsOperandName::CommareaPointer => CicsOperandName::CommareaPointer,
                HirCicsOperandName::Map => CicsOperandName::Map,
                HirCicsOperandName::Mapset => CicsOperandName::Mapset,
                HirCicsOperandName::DestId => CicsOperandName::DestId,
                HirCicsOperandName::DestIdLength => CicsOperandName::DestIdLength,
                HirCicsOperandName::Subaddress => CicsOperandName::Subaddress,
                HirCicsOperandName::Volume => CicsOperandName::Volume,
                HirCicsOperandName::VolumeLength => CicsOperandName::VolumeLength,
                HirCicsOperandName::NumRec => CicsOperandName::NumRec,
                HirCicsOperandName::Errterm => CicsOperandName::Errterm,
                HirCicsOperandName::RouteTitle => CicsOperandName::RouteTitle,
                HirCicsOperandName::RouteList => CicsOperandName::RouteList,
                HirCicsOperandName::Opclass => CicsOperandName::Opclass,
                HirCicsOperandName::KeyNumber => CicsOperandName::KeyNumber,
                HirCicsOperandName::Partnset => CicsOperandName::Partnset,
                HirCicsOperandName::ControlCursor => CicsOperandName::ControlCursor,
                HirCicsOperandName::Msr => CicsOperandName::Msr,
                HirCicsOperandName::Outpartn => CicsOperandName::Outpartn,
                HirCicsOperandName::Actpartn => CicsOperandName::Actpartn,
                HirCicsOperandName::Ldc => CicsOperandName::Ldc,
                HirCicsOperandName::Trailer => CicsOperandName::Trailer,
                HirCicsOperandName::Fmhparm => CicsOperandName::Fmhparm,
                HirCicsOperandName::Resource => CicsOperandName::Resource,
                HirCicsOperandName::Length => CicsOperandName::Length,
                HirCicsOperandName::MaxLifetime => CicsOperandName::MaxLifetime,
                HirCicsOperandName::Priority => CicsOperandName::Priority,
                HirCicsOperandName::UserCorrData => CicsOperandName::UserCorrData,
                HirCicsOperandName::SetAddress => CicsOperandName::SetAddress,
                HirCicsOperandName::SetPointer => CicsOperandName::SetPointer,
                HirCicsOperandName::UsingAddress => CicsOperandName::UsingAddress,
                HirCicsOperandName::UsingPointer => CicsOperandName::UsingPointer,
                HirCicsOperandName::Conditions => CicsOperandName::Conditions,
                HirCicsOperandName::Aids => CicsOperandName::Aids,
                HirCicsOperandName::Abstime => CicsOperandName::Abstime,
                HirCicsOperandName::DateString => CicsOperandName::DateString,
                HirCicsOperandName::Field => CicsOperandName::Field,
                HirCicsOperandName::Record => CicsOperandName::Record,
                HirCicsOperandName::RecordLength => CicsOperandName::RecordLength,
                HirCicsOperandName::DigestType => CicsOperandName::DigestType,
                HirCicsOperandName::OperatorText => CicsOperandName::OperatorText,
                HirCicsOperandName::OperatorTextLength => CicsOperandName::OperatorTextLength,
                HirCicsOperandName::OperatorRouteCodes => CicsOperandName::OperatorRouteCodes,
                HirCicsOperandName::OperatorNumRoutes => CicsOperandName::OperatorNumRoutes,
                HirCicsOperandName::OperatorConsName => CicsOperandName::OperatorConsName,
                HirCicsOperandName::OperatorAction => CicsOperandName::OperatorAction,
                HirCicsOperandName::OperatorMaxLength => CicsOperandName::OperatorMaxLength,
                HirCicsOperandName::OperatorTimeout => CicsOperandName::OperatorTimeout,
                HirCicsOperandName::DateSep => CicsOperandName::DateSep,
                HirCicsOperandName::TimeSep => CicsOperandName::TimeSep,
                HirCicsOperandName::KeyLength => CicsOperandName::KeyLength,
                HirCicsOperandName::ReqId => CicsOperandName::ReqId,
                HirCicsOperandName::Interval => CicsOperandName::Interval,
                HirCicsOperandName::StartTime => CicsOperandName::StartTime,
                HirCicsOperandName::Hours => CicsOperandName::Hours,
                HirCicsOperandName::Minutes => CicsOperandName::Minutes,
                HirCicsOperandName::Seconds => CicsOperandName::Seconds,
                HirCicsOperandName::Milliseconds => CicsOperandName::Milliseconds,
                HirCicsOperandName::DataLength => CicsOperandName::DataLength,
                HirCicsOperandName::Flength => CicsOperandName::Flength,
                HirCicsOperandName::InitImage => CicsOperandName::InitImage,
                HirCicsOperandName::DataPointer => CicsOperandName::DataPointer,
                HirCicsOperandName::DataArea => CicsOperandName::DataArea,
                HirCicsOperandName::EventControlAddress => CicsOperandName::EventControlAddress,
                HirCicsOperandName::WaitName => CicsOperandName::WaitName,
                HirCicsOperandName::EcbList => CicsOperandName::EcbList,
                HirCicsOperandName::NumEvents => CicsOperandName::NumEvents,
                HirCicsOperandName::Purgeability => CicsOperandName::Purgeability,
                HirCicsOperandName::Application => CicsOperandName::Application,
                HirCicsOperandName::Platform => CicsOperandName::Platform,
                HirCicsOperandName::ApplicationOperation => CicsOperandName::ApplicationOperation,
                HirCicsOperandName::MajorVersion => CicsOperandName::MajorVersion,
                HirCicsOperandName::MinorVersion => CicsOperandName::MinorVersion,
                HirCicsOperandName::Channel => CicsOperandName::Channel,
                HirCicsOperandName::LoadSet => CicsOperandName::LoadSet,
                HirCicsOperandName::Entry => CicsOperandName::Entry,
                HirCicsOperandName::LoadLength => CicsOperandName::LoadLength,
                HirCicsOperandName::LoadFlength => CicsOperandName::LoadFlength,
                HirCicsOperandName::DocumentToken => CicsOperandName::DocumentToken,
                HirCicsOperandName::Text => CicsOperandName::Text,
                HirCicsOperandName::Binary => CicsOperandName::Binary,
                HirCicsOperandName::FromDocument => CicsOperandName::FromDocument,
                HirCicsOperandName::Template => CicsOperandName::Template,
                HirCicsOperandName::SymbolList => CicsOperandName::SymbolList,
                HirCicsOperandName::ListLength => CicsOperandName::ListLength,
                HirCicsOperandName::Delimiter => CicsOperandName::Delimiter,
                HirCicsOperandName::HostCodePage => CicsOperandName::HostCodePage,
                HirCicsOperandName::Bookmark => CicsOperandName::Bookmark,
                HirCicsOperandName::Symbol => CicsOperandName::Symbol,
                HirCicsOperandName::AtBookmark => CicsOperandName::AtBookmark,
                HirCicsOperandName::ToBookmark => CicsOperandName::ToBookmark,
                HirCicsOperandName::MaximumLength => CicsOperandName::MaximumLength,
                HirCicsOperandName::CharacterSet => CicsOperandName::CharacterSet,
                HirCicsOperandName::SymbolValue => CicsOperandName::SymbolValue,
                HirCicsOperandName::Service => CicsOperandName::Service,
                HirCicsOperandName::ServiceOperation => CicsOperandName::ServiceOperation,
                HirCicsOperandName::Uri => CicsOperandName::Uri,
                HirCicsOperandName::UriMap => CicsOperandName::UriMap,
                HirCicsOperandName::Scope => CicsOperandName::Scope,
                HirCicsOperandName::ScopeLen => CicsOperandName::ScopeLen,
                HirCicsOperandName::FaultCode => CicsOperandName::FaultCode,
                HirCicsOperandName::FaultCodeStr => CicsOperandName::FaultCodeStr,
                HirCicsOperandName::FaultCodeLen => CicsOperandName::FaultCodeLen,
                HirCicsOperandName::FaultString => CicsOperandName::FaultString,
                HirCicsOperandName::FaultStrLen => CicsOperandName::FaultStrLen,
                HirCicsOperandName::NatLang => CicsOperandName::NatLang,
                HirCicsOperandName::SoapRole => CicsOperandName::SoapRole,
                HirCicsOperandName::RoleLength => CicsOperandName::RoleLength,
                HirCicsOperandName::FaultActor => CicsOperandName::FaultActor,
                HirCicsOperandName::FaultActLen => CicsOperandName::FaultActLen,
                HirCicsOperandName::Detail => CicsOperandName::Detail,
                HirCicsOperandName::DetailLength => CicsOperandName::DetailLength,
                HirCicsOperandName::FromCcsid => CicsOperandName::FromCcsid,
                HirCicsOperandName::SubcodeStr => CicsOperandName::SubcodeStr,
                HirCicsOperandName::SubcodeLen => CicsOperandName::SubcodeLen,
                HirCicsOperandName::ContextType => CicsOperandName::ContextType,
                HirCicsOperandName::Action => CicsOperandName::Action,
                HirCicsOperandName::MessageId => CicsOperandName::MessageId,
                HirCicsOperandName::RelatesUri => CicsOperandName::RelatesUri,
                HirCicsOperandName::RelatesType => CicsOperandName::RelatesType,
                HirCicsOperandName::RelatesIndex => CicsOperandName::RelatesIndex,
                HirCicsOperandName::EprType => CicsOperandName::EprType,
                HirCicsOperandName::EprField => CicsOperandName::EprField,
                HirCicsOperandName::EprFrom => CicsOperandName::EprFrom,
                HirCicsOperandName::EprLength => CicsOperandName::EprLength,
                HirCicsOperandName::FromCodepage => CicsOperandName::FromCodepage,
                HirCicsOperandName::IntoCcsid => CicsOperandName::IntoCcsid,
                HirCicsOperandName::IntoCodepage => CicsOperandName::IntoCodepage,
                HirCicsOperandName::Address => CicsOperandName::Address,
                HirCicsOperandName::RefParms => CicsOperandName::RefParms,
                HirCicsOperandName::RefParmsLen => CicsOperandName::RefParmsLen,
                HirCicsOperandName::Metadata => CicsOperandName::Metadata,
                HirCicsOperandName::MetadataLen => CicsOperandName::MetadataLen,
                HirCicsOperandName::InContainer => CicsOperandName::InContainer,
                HirCicsOperandName::OutContainer => CicsOperandName::OutContainer,
                HirCicsOperandName::Transformer => CicsOperandName::Transformer,
                HirCicsOperandName::DataContainer => CicsOperandName::DataContainer,
                HirCicsOperandName::XmlContainer => CicsOperandName::XmlContainer,
                HirCicsOperandName::XmlTransform => CicsOperandName::XmlTransform,
                HirCicsOperandName::NsContainer => CicsOperandName::NsContainer,
                HirCicsOperandName::ElementName => CicsOperandName::ElementName,
                HirCicsOperandName::ElementNameLength => CicsOperandName::ElementNameLength,
                HirCicsOperandName::ElementNamespace => CicsOperandName::ElementNamespace,
                HirCicsOperandName::ElementNamespaceLength => {
                    CicsOperandName::ElementNamespaceLength
                }
                HirCicsOperandName::TypeNameLength => CicsOperandName::TypeNameLength,
                HirCicsOperandName::TypeName => CicsOperandName::TypeName,
                HirCicsOperandName::TypeNamespace => CicsOperandName::TypeNamespace,
                HirCicsOperandName::TypeNamespaceLength => CicsOperandName::TypeNamespaceLength,
                HirCicsOperandName::CounterName => CicsOperandName::CounterName,
                HirCicsOperandName::CounterPool => CicsOperandName::CounterPool,
                HirCicsOperandName::CounterValue => CicsOperandName::CounterValue,
                HirCicsOperandName::CounterMinimum => CicsOperandName::CounterMinimum,
                HirCicsOperandName::CounterMaximum => CicsOperandName::CounterMaximum,
                HirCicsOperandName::CounterIncrement => CicsOperandName::CounterIncrement,
                HirCicsOperandName::CounterCompareMin => CicsOperandName::CounterCompareMin,
                HirCicsOperandName::CounterCompareMax => CicsOperandName::CounterCompareMax,
                HirCicsOperandName::JournalName => CicsOperandName::JournalName,
                HirCicsOperandName::JournalNum => CicsOperandName::JournalNum,
                HirCicsOperandName::JournalReqId => CicsOperandName::JournalReqId,
                HirCicsOperandName::JournalTypeId => CicsOperandName::JournalTypeId,
                HirCicsOperandName::JournalFrom => CicsOperandName::JournalFrom,
                HirCicsOperandName::JournalFlength => CicsOperandName::JournalFlength,
                HirCicsOperandName::JournalPrefix => CicsOperandName::JournalPrefix,
                HirCicsOperandName::JournalPfxLeng => CicsOperandName::JournalPfxLeng,
                HirCicsOperandName::SpoolToken => CicsOperandName::SpoolToken,
                HirCicsOperandName::SpoolUserId => CicsOperandName::SpoolUserId,
                HirCicsOperandName::SpoolClass => CicsOperandName::SpoolClass,
                HirCicsOperandName::SpoolNode => CicsOperandName::SpoolNode,
                HirCicsOperandName::SpoolRecordLength => CicsOperandName::SpoolRecordLength,
                HirCicsOperandName::SpoolOutDescr => CicsOperandName::SpoolOutDescr,
                HirCicsOperandName::SpoolMaxFlength => CicsOperandName::SpoolMaxFlength,
                HirCicsOperandName::SpoolFrom => CicsOperandName::SpoolFrom,
                HirCicsOperandName::SpoolFlength => CicsOperandName::SpoolFlength,
                HirCicsOperandName::TraceNum => CicsOperandName::TraceNum,
                HirCicsOperandName::TraceFrom => CicsOperandName::TraceFrom,
                HirCicsOperandName::TraceFromLength => CicsOperandName::TraceFromLength,
                HirCicsOperandName::TraceResource => CicsOperandName::TraceResource,
                HirCicsOperandName::MonitorPoint => CicsOperandName::MonitorPoint,
                HirCicsOperandName::MonitorEntryName => CicsOperandName::MonitorEntryName,
                HirCicsOperandName::MonitorData1 => CicsOperandName::MonitorData1,
                HirCicsOperandName::MonitorData2 => CicsOperandName::MonitorData2,
                HirCicsOperandName::DumpCode => CicsOperandName::DumpCode,
                HirCicsOperandName::DumpFrom => CicsOperandName::DumpFrom,
                HirCicsOperandName::DumpLength => CicsOperandName::DumpLength,
                HirCicsOperandName::DumpFlength => CicsOperandName::DumpFlength,
                HirCicsOperandName::DumpSegmentList => CicsOperandName::DumpSegmentList,
                HirCicsOperandName::DumpLengthList => CicsOperandName::DumpLengthList,
                HirCicsOperandName::DumpNumSegments => CicsOperandName::DumpNumSegments,
                HirCicsOperandName::TraceId => CicsOperandName::TraceId,
                HirCicsOperandName::TraceIdFrom => CicsOperandName::TraceIdFrom,
                HirCicsOperandName::TraceIdResource => CicsOperandName::TraceIdResource,
                HirCicsOperandName::TraceEntryName => CicsOperandName::TraceEntryName,
                HirCicsOperandName::WebUrl => CicsOperandName::WebUrl,
                HirCicsOperandName::WebUrlLength => CicsOperandName::WebUrlLength,
                HirCicsOperandName::WebHostLength => CicsOperandName::WebHostLength,
                HirCicsOperandName::WebPathLength => CicsOperandName::WebPathLength,
                HirCicsOperandName::WebQueryStringLength => CicsOperandName::WebQueryStringLength,
                HirCicsOperandName::WebHost => CicsOperandName::WebHost,
                HirCicsOperandName::WebSessionToken => CicsOperandName::WebSessionToken,
                HirCicsOperandName::WebMethodLength => CicsOperandName::WebMethodLength,
                HirCicsOperandName::WebVersionLength => CicsOperandName::WebVersionLength,
                HirCicsOperandName::WebRealmLength => CicsOperandName::WebRealmLength,
                HirCicsOperandName::WebHttpHeaderName => CicsOperandName::WebHttpHeaderName,
                HirCicsOperandName::WebQueryParmName => CicsOperandName::WebQueryParmName,
                HirCicsOperandName::WebFormFieldName => CicsOperandName::WebFormFieldName,
                HirCicsOperandName::WebNameLength => CicsOperandName::WebNameLength,
                HirCicsOperandName::WebValueLength => CicsOperandName::WebValueLength,
                HirCicsOperandName::WebBrowseStartName => CicsOperandName::WebBrowseStartName,
                HirCicsOperandName::WebHeaderValue => CicsOperandName::WebHeaderValue,
                HirCicsOperandName::WebMethod => CicsOperandName::WebMethod,
                HirCicsOperandName::WebAction => CicsOperandName::WebAction,
                HirCicsOperandName::WebCloseStatus => CicsOperandName::WebCloseStatus,
                HirCicsOperandName::WebDocumentToken => CicsOperandName::WebDocumentToken,
                HirCicsOperandName::WebStatusCode => CicsOperandName::WebStatusCode,
                HirCicsOperandName::WebStatusText => CicsOperandName::WebStatusText,
                HirCicsOperandName::WebStatusLength => CicsOperandName::WebStatusLength,
                HirCicsOperandName::WebFrom => CicsOperandName::WebFrom,
                HirCicsOperandName::WebFromLength => CicsOperandName::WebFromLength,
                HirCicsOperandName::WebPathInput => CicsOperandName::WebPathInput,
                HirCicsOperandName::WebQueryInput => CicsOperandName::WebQueryInput,
                HirCicsOperandName::WebMediaType => CicsOperandName::WebMediaType,
                HirCicsOperandName::WebSendUriMap => CicsOperandName::WebSendUriMap,
                HirCicsOperandName::WebReceiveMaxLength => CicsOperandName::WebReceiveMaxLength,
                HirCicsOperandName::WebReceiveStatusLength => {
                    CicsOperandName::WebReceiveStatusLength
                }
                HirCicsOperandName::WebPortNumber => CicsOperandName::WebPortNumber,
                HirCicsOperandName::WebScheme => CicsOperandName::WebScheme,
                HirCicsOperandName::WebUriMap => CicsOperandName::WebUriMap,
                HirCicsOperandName::WebCertificate => CicsOperandName::WebCertificate,
                HirCicsOperandName::WebCodePage => CicsOperandName::WebCodePage,
            },
            value: match &operand.value {
                HirCicsValue::Literal(value) => {
                    CicsOperandValue::Literal(value.as_bytes().to_vec())
                }
                HirCicsValue::Data(reference) => CicsOperandValue::Storage(self.slot(reference)?),
                HirCicsValue::Integer(value) => CicsOperandValue::Integer(*value),
                HirCicsValue::LengthOf(reference) => {
                    CicsOperandValue::LengthOf(self.slot(reference)?)
                }
            },
        })
    }

    fn output(
        &mut self,
        output: &HirCicsOutputBinding,
    ) -> Result<CicsOutputBinding, CicsPlanProblem> {
        Ok(CicsOutputBinding {
            name: match output.name {
                HirCicsOutputName::ConversationState => CicsOutputName::ConversationState,
                HirCicsOutputName::ConversationConvid => CicsOutputName::ConversationConvid,
                HirCicsOutputName::ConversationRetcode => CicsOutputName::ConversationRetcode,
                HirCicsOutputName::ConversationPrinConvid => CicsOutputName::ConversationPrinConvid,
                HirCicsOutputName::ConversationPrinSysid => CicsOutputName::ConversationPrinSysid,
                HirCicsOutputName::ConversationConvData => CicsOutputName::ConversationConvData,
                HirCicsOutputName::ConversationInto => CicsOutputName::ConversationInto,
                HirCicsOutputName::ConversationSet => CicsOutputName::ConversationSet,
                HirCicsOutputName::ConversationToLength => CicsOutputName::ConversationToLength,
                HirCicsOutputName::ConversationToFullLength => {
                    CicsOutputName::ConversationToFullLength
                }
                HirCicsOutputName::Abstime => CicsOutputName::Abstime,
                HirCicsOutputName::SecurityRead => CicsOutputName::SecurityRead,
                HirCicsOutputName::IssueState => CicsOutputName::IssueState,
                HirCicsOutputName::SecurityUpdate => CicsOutputName::SecurityUpdate,
                HirCicsOutputName::SecurityControl => CicsOutputName::SecurityControl,
                HirCicsOutputName::SecurityAlter => CicsOutputName::SecurityAlter,
                HirCicsOutputName::SecurityChangeTime => CicsOutputName::SecurityChangeTime,
                HirCicsOutputName::SecurityDaysLeft => CicsOutputName::SecurityDaysLeft,
                HirCicsOutputName::SecurityEsmReason => CicsOutputName::SecurityEsmReason,
                HirCicsOutputName::SecurityEsmResp => CicsOutputName::SecurityEsmResp,
                HirCicsOutputName::SecurityExpiryTime => CicsOutputName::SecurityExpiryTime,
                HirCicsOutputName::SecurityInvalidCount => CicsOutputName::SecurityInvalidCount,
                HirCicsOutputName::SecurityLastUseTime => CicsOutputName::SecurityLastUseTime,
                HirCicsOutputName::SecurityPassTicket => CicsOutputName::SecurityPassTicket,
                HirCicsOutputName::SecurityIsUserId => CicsOutputName::SecurityIsUserId,
                HirCicsOutputName::SecurityEncryptKey => CicsOutputName::SecurityEncryptKey,
                HirCicsOutputName::SecurityOutToken => CicsOutputName::SecurityOutToken,
                HirCicsOutputName::SecurityOutTokenLength => CicsOutputName::SecurityOutTokenLength,
                HirCicsOutputName::SecurityEncryptPassTicket => {
                    CicsOutputName::SecurityEncryptPassTicket
                }
                HirCicsOutputName::SecurityEncryptLength => CicsOutputName::SecurityEncryptLength,
                HirCicsOutputName::SecurityLangInUse => CicsOutputName::SecurityLangInUse,
                HirCicsOutputName::SecurityNatLangInUse => CicsOutputName::SecurityNatLangInUse,
                HirCicsOutputName::Field => CicsOutputName::Field,
                HirCicsOutputName::DigestResult => CicsOutputName::DigestResult,
                HirCicsOutputName::OperatorReply => CicsOutputName::OperatorReply,
                HirCicsOutputName::OperatorReplyLength => CicsOutputName::OperatorReplyLength,
                HirCicsOutputName::Certificate(output) => CicsOutputName::Certificate(output),
                HirCicsOutputName::Tcpip(output) => CicsOutputName::Tcpip(output),
                HirCicsOutputName::Commarea => CicsOutputName::Commarea,
                HirCicsOutputName::Into => CicsOutputName::Into,
                HirCicsOutputName::Partn => CicsOutputName::Partn,
                HirCicsOutputName::SetPointer => CicsOutputName::SetPointer,
                HirCicsOutputName::Ridfld => CicsOutputName::Ridfld,
                HirCicsOutputName::AttachProcess => CicsOutputName::AttachProcess,
                HirCicsOutputName::AttachResource => CicsOutputName::AttachResource,
                HirCicsOutputName::AttachReturnProcess => CicsOutputName::AttachReturnProcess,
                HirCicsOutputName::AttachReturnResource => CicsOutputName::AttachReturnResource,
                HirCicsOutputName::AttachQueue => CicsOutputName::AttachQueue,
                HirCicsOutputName::AttachIuType => CicsOutputName::AttachIuType,
                HirCicsOutputName::AttachDataStream => CicsOutputName::AttachDataStream,
                HirCicsOutputName::AttachRecordFormat => CicsOutputName::AttachRecordFormat,
                HirCicsOutputName::ConversationData => CicsOutputName::ConversationData,
                HirCicsOutputName::ConversationRetCode => CicsOutputName::ConversationRetCode,
                HirCicsOutputName::LogonInto => CicsOutputName::LogonInto,
                HirCicsOutputName::LogonSet => CicsOutputName::LogonSet,
                HirCicsOutputName::LogonLength => CicsOutputName::LogonLength,
                HirCicsOutputName::ProcessName => CicsOutputName::ProcessName,
                HirCicsOutputName::ProcessLength => CicsOutputName::ProcessLength,
                HirCicsOutputName::SyncLevel => CicsOutputName::SyncLevel,
                HirCicsOutputName::PipList => CicsOutputName::PipList,
                HirCicsOutputName::PipLength => CicsOutputName::PipLength,
                HirCicsOutputName::TctSysId => CicsOutputName::TctSysId,
                HirCicsOutputName::TctTermId => CicsOutputName::TctTermId,
                HirCicsOutputName::Token => CicsOutputName::Token,
                HirCicsOutputName::Milliseconds => CicsOutputName::Milliseconds,
                HirCicsOutputName::Mmddyy => CicsOutputName::Mmddyy,
                HirCicsOutputName::Mmddyyyy => CicsOutputName::Mmddyyyy,
                HirCicsOutputName::Resp => CicsOutputName::Resp,
                HirCicsOutputName::TimerStatus => CicsOutputName::TimerStatus,
                HirCicsOutputName::BtsAny => CicsOutputName::BtsAny,
                HirCicsOutputName::BtsCompStatus => CicsOutputName::BtsCompStatus,
                HirCicsOutputName::BtsChannel => CicsOutputName::BtsChannel,
                HirCicsOutputName::BtsAbcode => CicsOutputName::BtsAbcode,
                HirCicsOutputName::EventName => CicsOutputName::EventName,
                HirCicsOutputName::SubEventName => CicsOutputName::SubEventName,
                HirCicsOutputName::EventType => CicsOutputName::EventType,
                HirCicsOutputName::FireStatus => CicsOutputName::FireStatus,
                HirCicsOutputName::Resp2 => CicsOutputName::Resp2,
                HirCicsOutputName::Time => CicsOutputName::Time,
                HirCicsOutputName::Yyddd => CicsOutputName::Yyddd,
                HirCicsOutputName::Yymmdd => CicsOutputName::Yymmdd,
                HirCicsOutputName::Yyyymmdd => CicsOutputName::Yyyymmdd,
                HirCicsOutputName::Assign(output) => CicsOutputName::Assign(output),
                HirCicsOutputName::Length => CicsOutputName::Length,
                HirCicsOutputName::ReturnTransId => CicsOutputName::ReturnTransId,
                HirCicsOutputName::ReturnTermId => CicsOutputName::ReturnTermId,
                HirCicsOutputName::Queue => CicsOutputName::Queue,
                HirCicsOutputName::NumItems => CicsOutputName::NumItems,
                HirCicsOutputName::DocumentToken => CicsOutputName::DocumentToken,
                HirCicsOutputName::DocumentSize => CicsOutputName::DocumentSize,
                HirCicsOutputName::WebAction => CicsOutputName::WebAction,
                HirCicsOutputName::WebMessageId => CicsOutputName::WebMessageId,
                HirCicsOutputName::WebRelatesUri => CicsOutputName::WebRelatesUri,
                HirCicsOutputName::WebRelatesType => CicsOutputName::WebRelatesType,
                HirCicsOutputName::WebEprInto => CicsOutputName::WebEprInto,
                HirCicsOutputName::WebEprSet => CicsOutputName::WebEprSet,
                HirCicsOutputName::WebEprLength => CicsOutputName::WebEprLength,
                HirCicsOutputName::ElementName => CicsOutputName::ElementName,
                HirCicsOutputName::ElementNameLength => CicsOutputName::ElementNameLength,
                HirCicsOutputName::ElementNamespace => CicsOutputName::ElementNamespace,
                HirCicsOutputName::ElementNamespaceLength => CicsOutputName::ElementNamespaceLength,
                HirCicsOutputName::TypeName => CicsOutputName::TypeName,
                HirCicsOutputName::TypeNameLength => CicsOutputName::TypeNameLength,
                HirCicsOutputName::TypeNamespace => CicsOutputName::TypeNamespace,
                HirCicsOutputName::TypeNamespaceLength => CicsOutputName::TypeNamespaceLength,
                HirCicsOutputName::JournalReqId => CicsOutputName::JournalReqId,
                HirCicsOutputName::CounterValue => CicsOutputName::CounterValue,
                HirCicsOutputName::CounterMinimum => CicsOutputName::CounterMinimum,
                HirCicsOutputName::CounterMaximum => CicsOutputName::CounterMaximum,
                HirCicsOutputName::SpoolToken => CicsOutputName::SpoolToken,
                HirCicsOutputName::SpoolToFlength => CicsOutputName::SpoolToFlength,
                HirCicsOutputName::DumpId => CicsOutputName::DumpId,
                HirCicsOutputName::WebSchemeName => CicsOutputName::WebSchemeName,
                HirCicsOutputName::WebHost => CicsOutputName::WebHost,
                HirCicsOutputName::WebHostLength => CicsOutputName::WebHostLength,
                HirCicsOutputName::WebHostType => CicsOutputName::WebHostType,
                HirCicsOutputName::WebPortNumber => CicsOutputName::WebPortNumber,
                HirCicsOutputName::WebPath => CicsOutputName::WebPath,
                HirCicsOutputName::WebPathLength => CicsOutputName::WebPathLength,
                HirCicsOutputName::WebQueryString => CicsOutputName::WebQueryString,
                HirCicsOutputName::WebQueryStringLength => CicsOutputName::WebQueryStringLength,
                HirCicsOutputName::WebSessionToken => CicsOutputName::WebSessionToken,
                HirCicsOutputName::WebHttpVNum => CicsOutputName::WebHttpVNum,
                HirCicsOutputName::WebHttpRNum => CicsOutputName::WebHttpRNum,
                HirCicsOutputName::WebScheme => CicsOutputName::WebScheme,
                HirCicsOutputName::WebHttpMethod => CicsOutputName::WebHttpMethod,
                HirCicsOutputName::WebMethodLength => CicsOutputName::WebMethodLength,
                HirCicsOutputName::WebHttpVersion => CicsOutputName::WebHttpVersion,
                HirCicsOutputName::WebVersionLength => CicsOutputName::WebVersionLength,
                HirCicsOutputName::WebRequestType => CicsOutputName::WebRequestType,
                HirCicsOutputName::WebUriMap => CicsOutputName::WebUriMap,
                HirCicsOutputName::WebRealm => CicsOutputName::WebRealm,
                HirCicsOutputName::WebRealmLength => CicsOutputName::WebRealmLength,
                HirCicsOutputName::WebValue => CicsOutputName::WebValue,
                HirCicsOutputName::WebValueLength => CicsOutputName::WebValueLength,
                HirCicsOutputName::WebBrowseName => CicsOutputName::WebBrowseName,
                HirCicsOutputName::WebBrowseNameLength => CicsOutputName::WebBrowseNameLength,
                HirCicsOutputName::WebRetrieveDocumentToken => {
                    CicsOutputName::WebRetrieveDocumentToken
                }
                HirCicsOutputName::WebReceiveInto => CicsOutputName::WebReceiveInto,
                HirCicsOutputName::WebReceiveLength => CicsOutputName::WebReceiveLength,
                HirCicsOutputName::WebReceiveStatusCode => CicsOutputName::WebReceiveStatusCode,
                HirCicsOutputName::WebReceiveStatusText => CicsOutputName::WebReceiveStatusText,
                HirCicsOutputName::WebReceiveStatusLength => CicsOutputName::WebReceiveStatusLength,
                HirCicsOutputName::WebReceiveMediaType => CicsOutputName::WebReceiveMediaType,
                HirCicsOutputName::WebReceiveBodyCharset => CicsOutputName::WebReceiveBodyCharset,
                HirCicsOutputName::WebConverseInto => CicsOutputName::WebConverseInto,
                HirCicsOutputName::WebConverseToLength => CicsOutputName::WebConverseToLength,
                HirCicsOutputName::WebConverseStatusCode => CicsOutputName::WebConverseStatusCode,
                HirCicsOutputName::WebConverseStatusText => CicsOutputName::WebConverseStatusText,
                HirCicsOutputName::WebConverseStatusLength => {
                    CicsOutputName::WebConverseStatusLength
                }
                HirCicsOutputName::WebConverseMediaType => CicsOutputName::WebConverseMediaType,
                HirCicsOutputName::WebConverseBodyCharset => CicsOutputName::WebConverseBodyCharset,
            },
            target: self.slot(&output.target)?,
        })
    }

    fn slot(&mut self, reference: &HirDataReference) -> Result<CicsStorageSlot, CicsPlanProblem> {
        let storage = *self
            .storage_ids
            .get(&reference.qualified_name)
            .ok_or_else(|| CicsPlanProblem::MissingStorage(reference.qualified_name.clone()))?;
        let length = if reference.dynamic {
            reference.dynamic_limit.unwrap_or(reference.length)
        } else {
            reference.length
        };
        if length == 0 {
            return Err(CicsPlanProblem::InvalidStorageExtent(
                reference.qualified_name.clone(),
            ));
        }
        let declared = StorageReference {
            storage,
            offset: 0,
            length: length as u64,
        };
        if self
            .references
            .insert(storage, declared.clone())
            .is_some_and(|existing| existing != declared)
        {
            return Err(CicsPlanProblem::InvalidStorageExtent(
                reference.qualified_name.clone(),
            ));
        }
        Ok(CicsStorageSlot {
            storage,
            qualified_layout_name: reference.qualified_name.clone(),
        })
    }
}

const fn plan_operation(operation: HirCicsOperation) -> CicsPlanOperation {
    match operation {
        HirCicsOperation::FetchAny => CicsPlanOperation::FetchAny,
        HirCicsOperation::FetchChild => CicsPlanOperation::FetchChild,
        HirCicsOperation::FreeChild => CicsPlanOperation::FreeChild,
        HirCicsOperation::LinkAcqActivity => CicsPlanOperation::LinkAcqActivity,
        HirCicsOperation::LinkAcqProcess => CicsPlanOperation::LinkAcqProcess,
        HirCicsOperation::LinkActivity => CicsPlanOperation::LinkActivity,
        HirCicsOperation::AllocateConversation => CicsPlanOperation::AllocateConversation,
        HirCicsOperation::GdsAllocateConversation => CicsPlanOperation::GdsAllocateConversation,
        HirCicsOperation::GdsAssignConversation => CicsPlanOperation::GdsAssignConversation,
        HirCicsOperation::BuildAttach => CicsPlanOperation::BuildAttach,
        HirCicsOperation::ConnectProcess => CicsPlanOperation::ConnectProcess,
        HirCicsOperation::GdsConnectProcess => CicsPlanOperation::GdsConnectProcess,
        HirCicsOperation::Converse => CicsPlanOperation::Converse,
        HirCicsOperation::FreeConversation => CicsPlanOperation::FreeConversation,
        HirCicsOperation::GdsFreeConversation => CicsPlanOperation::GdsFreeConversation,
        HirCicsOperation::Abend => CicsPlanOperation::Abend,
        HirCicsOperation::QuerySecurity => CicsPlanOperation::QuerySecurity,
        HirCicsOperation::VerifyPassword => CicsPlanOperation::VerifyPassword,
        HirCicsOperation::ChangePassword => CicsPlanOperation::ChangePassword,
        HirCicsOperation::ChangePhrase => CicsPlanOperation::ChangePhrase,
        HirCicsOperation::RequestPassTicket => CicsPlanOperation::RequestPassTicket,
        HirCicsOperation::RequestEncryptPassTicket => CicsPlanOperation::RequestEncryptPassTicket,
        HirCicsOperation::Signon => CicsPlanOperation::Signon,
        HirCicsOperation::ExtractAttach => CicsPlanOperation::ExtractAttach,
        HirCicsOperation::ExtractAttributes => CicsPlanOperation::ExtractAttributes,
        HirCicsOperation::GdsExtractAttributes => CicsPlanOperation::GdsExtractAttributes,
        HirCicsOperation::ExtractLogonMsg => CicsPlanOperation::ExtractLogonMsg,
        HirCicsOperation::ExtractProcess => CicsPlanOperation::ExtractProcess,
        HirCicsOperation::GdsExtractProcess => CicsPlanOperation::GdsExtractProcess,
        HirCicsOperation::ExtractTct => CicsPlanOperation::ExtractTct,
        HirCicsOperation::Point => CicsPlanOperation::Point,
        HirCicsOperation::Signoff => CicsPlanOperation::Signoff,
        HirCicsOperation::VerifyPhrase => CicsPlanOperation::VerifyPhrase,
        HirCicsOperation::VerifyToken => CicsPlanOperation::VerifyToken,
        HirCicsOperation::Address => CicsPlanOperation::Address,
        HirCicsOperation::AddressSet => CicsPlanOperation::AddressSet,
        HirCicsOperation::Asktime => CicsPlanOperation::Asktime,
        HirCicsOperation::AsktimeEib => CicsPlanOperation::AsktimeEib,
        HirCicsOperation::FormatTime => CicsPlanOperation::FormatTime,
        HirCicsOperation::ConvertTime => CicsPlanOperation::ConvertTime,
        HirCicsOperation::BifDeedit => CicsPlanOperation::BifDeedit,
        HirCicsOperation::BifDigest => CicsPlanOperation::BifDigest,
        HirCicsOperation::Cancel => CicsPlanOperation::Cancel,
        HirCicsOperation::Delay => CicsPlanOperation::Delay,
        HirCicsOperation::DefineCounter => CicsPlanOperation::DefineCounter,
        HirCicsOperation::DefineDCounter => CicsPlanOperation::DefineDCounter,
        HirCicsOperation::DeleteCounter => CicsPlanOperation::DeleteCounter,
        HirCicsOperation::DeleteDCounter => CicsPlanOperation::DeleteDCounter,
        HirCicsOperation::GetCounter => CicsPlanOperation::GetCounter,
        HirCicsOperation::GetDCounter => CicsPlanOperation::GetDCounter,
        HirCicsOperation::QueryCounter => CicsPlanOperation::QueryCounter,
        HirCicsOperation::QueryDCounter => CicsPlanOperation::QueryDCounter,
        HirCicsOperation::RewindCounter => CicsPlanOperation::RewindCounter,
        HirCicsOperation::RewindDCounter => CicsPlanOperation::RewindDCounter,
        HirCicsOperation::UpdateCounter => CicsPlanOperation::UpdateCounter,
        HirCicsOperation::UpdateDCounter => CicsPlanOperation::UpdateDCounter,
        HirCicsOperation::Post => CicsPlanOperation::Post,
        HirCicsOperation::WriteOperator => CicsPlanOperation::WriteOperator,
        HirCicsOperation::ExtractCertificate => CicsPlanOperation::ExtractCertificate,
        HirCicsOperation::ExtractTcpip => CicsPlanOperation::ExtractTcpip,
        HirCicsOperation::ChangeTask => CicsPlanOperation::ChangeTask,
        HirCicsOperation::Read => CicsPlanOperation::Read,
        HirCicsOperation::Rewrite => CicsPlanOperation::Rewrite,
        HirCicsOperation::SetAssociationUserCorrData => {
            CicsPlanOperation::SetAssociationUserCorrData
        }
        HirCicsOperation::SpoolClose => CicsPlanOperation::SpoolClose,
        HirCicsOperation::SpoolOpenInput => CicsPlanOperation::SpoolOpenInput,
        HirCicsOperation::SpoolOpenOutput => CicsPlanOperation::SpoolOpenOutput,
        HirCicsOperation::SpoolRead => CicsPlanOperation::SpoolRead,
        HirCicsOperation::SpoolWrite => CicsPlanOperation::SpoolWrite,
        HirCicsOperation::EnterTraceNum => CicsPlanOperation::EnterTraceNum,
        HirCicsOperation::Monitor => CicsPlanOperation::Monitor,
        HirCicsOperation::DumpTransaction => CicsPlanOperation::DumpTransaction,
        HirCicsOperation::Dump => CicsPlanOperation::Dump,
        HirCicsOperation::Trace => CicsPlanOperation::Trace,
        HirCicsOperation::EnterTraceId => CicsPlanOperation::EnterTraceId,
        HirCicsOperation::Syncpoint => CicsPlanOperation::Syncpoint,
        HirCicsOperation::Suspend => CicsPlanOperation::Suspend,
        HirCicsOperation::WaitEvent => CicsPlanOperation::WaitEvent,
        HirCicsOperation::WaitExternal => CicsPlanOperation::WaitExternal,
        HirCicsOperation::WaitCics => CicsPlanOperation::WaitCics,
        HirCicsOperation::Deq => CicsPlanOperation::Deq,
        HirCicsOperation::Enq => CicsPlanOperation::Enq,
        HirCicsOperation::HandleAid => CicsPlanOperation::HandleAid,
        HirCicsOperation::HandleAbend => CicsPlanOperation::HandleAbend,
        HirCicsOperation::HandleCondition => CicsPlanOperation::HandleCondition,
        HirCicsOperation::IgnoreCondition => CicsPlanOperation::IgnoreCondition,
        HirCicsOperation::InvokeApplication => CicsPlanOperation::InvokeApplication,
        HirCicsOperation::IssueAbort => CicsPlanOperation::IssueAbort,
        HirCicsOperation::IssueAdd => CicsPlanOperation::IssueAdd,
        HirCicsOperation::IssueEnd => CicsPlanOperation::IssueEnd,
        HirCicsOperation::IssueErase => CicsPlanOperation::IssueErase,
        HirCicsOperation::IssueNote => CicsPlanOperation::IssueNote,
        HirCicsOperation::IssueQuery => CicsPlanOperation::IssueQuery,
        HirCicsOperation::IssueReceive => CicsPlanOperation::IssueReceive,
        HirCicsOperation::IssueReplace => CicsPlanOperation::IssueReplace,
        HirCicsOperation::IssueSend => CicsPlanOperation::IssueSend,
        HirCicsOperation::Route => CicsPlanOperation::Route,
        HirCicsOperation::IssueWait => CicsPlanOperation::IssueWait,
        HirCicsOperation::IssueAbend => CicsPlanOperation::IssueAbend,
        HirCicsOperation::IssueConfirmation => CicsPlanOperation::IssueConfirmation,
        HirCicsOperation::IssueCopy => CicsPlanOperation::IssueCopy,
        HirCicsOperation::IssueDisconnect => CicsPlanOperation::IssueDisconnect,
        HirCicsOperation::IssueEndfile => CicsPlanOperation::IssueEndfile,
        HirCicsOperation::IssueEndoutput => CicsPlanOperation::IssueEndoutput,
        HirCicsOperation::IssueEods => CicsPlanOperation::IssueEods,
        HirCicsOperation::IssueEraseAup => CicsPlanOperation::IssueEraseAup,
        HirCicsOperation::IssueError => CicsPlanOperation::IssueError,
        HirCicsOperation::IssueLoad => CicsPlanOperation::IssueLoad,
        HirCicsOperation::IssuePass => CicsPlanOperation::IssuePass,
        HirCicsOperation::IssuePrepare => CicsPlanOperation::IssuePrepare,
        HirCicsOperation::IssuePrint => CicsPlanOperation::IssuePrint,
        HirCicsOperation::IssueReset => CicsPlanOperation::IssueReset,
        HirCicsOperation::IssueSignal => CicsPlanOperation::IssueSignal,
        HirCicsOperation::Load => CicsPlanOperation::Load,
        HirCicsOperation::Release => CicsPlanOperation::Release,
        HirCicsOperation::Link => CicsPlanOperation::Link,
        HirCicsOperation::Xctl => CicsPlanOperation::Xctl,
        HirCicsOperation::Return => CicsPlanOperation::Return,
        HirCicsOperation::StartBrowse => CicsPlanOperation::StartBrowse,
        HirCicsOperation::ResetBrowse => CicsPlanOperation::ResetBrowse,
        HirCicsOperation::Unlock => CicsPlanOperation::Unlock,
        HirCicsOperation::ReadNext => CicsPlanOperation::ReadNext,
        HirCicsOperation::ReadPrev => CicsPlanOperation::ReadPrev,
        HirCicsOperation::ReadTransientData => CicsPlanOperation::ReadTransientData,
        HirCicsOperation::EndBrowse => CicsPlanOperation::EndBrowse,
        HirCicsOperation::Delete => CicsPlanOperation::Delete,
        HirCicsOperation::Write => CicsPlanOperation::Write,
        HirCicsOperation::WriteTransientData => CicsPlanOperation::WriteTransientData,
        HirCicsOperation::DeleteTransientData => CicsPlanOperation::DeleteTransientData,
        HirCicsOperation::DeleteTemporaryStorage => CicsPlanOperation::DeleteTemporaryStorage,
        HirCicsOperation::ReadTemporaryStorage => CicsPlanOperation::ReadTemporaryStorage,
        HirCicsOperation::WriteTemporaryStorage => CicsPlanOperation::WriteTemporaryStorage,
        HirCicsOperation::Getmain => CicsPlanOperation::Getmain,
        HirCicsOperation::Freemain => CicsPlanOperation::Freemain,
        HirCicsOperation::ReceiveMap => CicsPlanOperation::ReceiveMap,
        HirCicsOperation::SendMap => CicsPlanOperation::SendMap,
        HirCicsOperation::SendText => CicsPlanOperation::SendText,
        HirCicsOperation::SendPartnset => CicsPlanOperation::SendPartnset,
        HirCicsOperation::SendControl => CicsPlanOperation::SendControl,
        HirCicsOperation::SendPage => CicsPlanOperation::SendPage,
        HirCicsOperation::ReceivePartn => CicsPlanOperation::ReceivePartn,
        HirCicsOperation::Assign => CicsPlanOperation::Assign,
        HirCicsOperation::PurgeMessage => CicsPlanOperation::PurgeMessage,
        HirCicsOperation::Start => CicsPlanOperation::Start,
        HirCicsOperation::StartAttach => CicsPlanOperation::StartAttach,
        HirCicsOperation::StartBrexit => CicsPlanOperation::StartBrexit,
        HirCicsOperation::Retrieve => CicsPlanOperation::Retrieve,
        HirCicsOperation::PopHandle => CicsPlanOperation::PopHandle,
        HirCicsOperation::PushHandle => CicsPlanOperation::PushHandle,
        HirCicsOperation::DocumentCreate => CicsPlanOperation::DocumentCreate,
        HirCicsOperation::DefineInputEvent => CicsPlanOperation::DefineInputEvent,
        HirCicsOperation::AddSubevent => CicsPlanOperation::AddSubevent,
        HirCicsOperation::RemoveSubevent => CicsPlanOperation::RemoveSubevent,
        HirCicsOperation::DeleteEvent => CicsPlanOperation::DeleteEvent,
        HirCicsOperation::CheckTimer => CicsPlanOperation::CheckTimer,
        HirCicsOperation::DefineTimer => CicsPlanOperation::DefineTimer,
        HirCicsOperation::DeleteTimer => CicsPlanOperation::DeleteTimer,
        HirCicsOperation::RetrieveReattachEvent => CicsPlanOperation::RetrieveReattachEvent,
        HirCicsOperation::RetrieveSubevent => CicsPlanOperation::RetrieveSubevent,
        HirCicsOperation::TestEvent => CicsPlanOperation::TestEvent,
        HirCicsOperation::SignalEvent => CicsPlanOperation::SignalEvent,
        HirCicsOperation::ForceTimer => CicsPlanOperation::ForceTimer,
        HirCicsOperation::DefineCompositeEvent => CicsPlanOperation::DefineCompositeEvent,
        HirCicsOperation::DocumentDelete => CicsPlanOperation::DocumentDelete,
        HirCicsOperation::DocumentInsert => CicsPlanOperation::DocumentInsert,
        HirCicsOperation::DocumentRetrieve => CicsPlanOperation::DocumentRetrieve,
        HirCicsOperation::DocumentSet => CicsPlanOperation::DocumentSet,
        HirCicsOperation::InvokeService => CicsPlanOperation::InvokeService,
        HirCicsOperation::SoapFaultAdd => CicsPlanOperation::SoapFaultAdd,
        HirCicsOperation::SoapFaultCreate => CicsPlanOperation::SoapFaultCreate,
        HirCicsOperation::SoapFaultDelete => CicsPlanOperation::SoapFaultDelete,
        HirCicsOperation::WsaContextBuild => CicsPlanOperation::WsaContextBuild,
        HirCicsOperation::WsaContextDelete => CicsPlanOperation::WsaContextDelete,
        HirCicsOperation::WsaContextGet => CicsPlanOperation::WsaContextGet,
        HirCicsOperation::WsaEprCreate => CicsPlanOperation::WsaEprCreate,
        HirCicsOperation::TransformDataToJson => CicsPlanOperation::TransformDataToJson,
        HirCicsOperation::TransformDataToXml => CicsPlanOperation::TransformDataToXml,
        HirCicsOperation::TransformJsonToData => CicsPlanOperation::TransformJsonToData,
        HirCicsOperation::TransformXmlToData => CicsPlanOperation::TransformXmlToData,
        HirCicsOperation::WebParseUrl => CicsPlanOperation::WebParseUrl,
        HirCicsOperation::WebOpen => CicsPlanOperation::WebOpen,
        HirCicsOperation::WebClose => CicsPlanOperation::WebClose,
        HirCicsOperation::WebExtract => CicsPlanOperation::WebExtract,
        HirCicsOperation::ExtractWeb => CicsPlanOperation::ExtractWeb,
        HirCicsOperation::WebRead => CicsPlanOperation::WebRead,
        HirCicsOperation::WebStartBrowse => CicsPlanOperation::WebStartBrowse,
        HirCicsOperation::WebReadNext => CicsPlanOperation::WebReadNext,
        HirCicsOperation::WebEndBrowse => CicsPlanOperation::WebEndBrowse,
        HirCicsOperation::WebWrite => CicsPlanOperation::WebWrite,
        HirCicsOperation::WebSend => CicsPlanOperation::WebSend,
        HirCicsOperation::WebRetrieve => CicsPlanOperation::WebRetrieve,
        HirCicsOperation::WebReceive => CicsPlanOperation::WebReceive,
        HirCicsOperation::WebConverse => CicsPlanOperation::WebConverse,
        HirCicsOperation::WaitJournalName => CicsPlanOperation::WaitJournalName,
        HirCicsOperation::WaitJournalNum => CicsPlanOperation::WaitJournalNum,
        HirCicsOperation::WriteJournalName => CicsPlanOperation::WriteJournalName,
        HirCicsOperation::WriteJournalNum => CicsPlanOperation::WriteJournalNum,
    }
}

const fn plan_option(option: HirCicsOption) -> CicsPlanOption {
    match option {
        HirCicsOption::BtsNoSuspend => CicsPlanOption::BtsNoSuspend,
        HirCicsOption::BtsAcqActivity => CicsPlanOption::BtsAcqActivity,
        HirCicsOption::BtsAcqProcess => CicsPlanOption::BtsAcqProcess,
        HirCicsOption::ConversationNoQueue => CicsPlanOption::ConversationNoQueue,
        HirCicsOption::ConversationNotruncate => CicsPlanOption::ConversationNotruncate,
        HirCicsOption::ConversationDefresp => CicsPlanOption::ConversationDefresp,
        HirCicsOption::ConversationFmh => CicsPlanOption::ConversationFmh,
        HirCicsOption::SecurityBasicAuth => CicsPlanOption::SecurityBasicAuth,
        HirCicsOption::SecurityJwt => CicsPlanOption::SecurityJwt,
        HirCicsOption::SecurityKerberos => CicsPlanOption::SecurityKerberos,
        HirCicsOption::SecurityBit => CicsPlanOption::SecurityBit,
        HirCicsOption::SecurityBase64 => CicsPlanOption::SecurityBase64,
        HirCicsOption::DigestHex => CicsPlanOption::DigestHex,
        HirCicsOption::DigestBinary => CicsPlanOption::DigestBinary,
        HirCicsOption::DigestBase64 => CicsPlanOption::DigestBase64,
        HirCicsOption::OperatorImmediate => CicsPlanOption::OperatorImmediate,
        HirCicsOption::OperatorEventual => CicsPlanOption::OperatorEventual,
        HirCicsOption::OperatorCritical => CicsPlanOption::OperatorCritical,
        HirCicsOption::CertificateOwner => CicsPlanOption::CertificateOwner,
        HirCicsOption::CertificateIssuer => CicsPlanOption::CertificateIssuer,
        HirCicsOption::Cancel => CicsPlanOption::Cancel,
        HirCicsOption::AsIs => CicsPlanOption::AsIs,
        HirCicsOption::Accum => CicsPlanOption::Accum,
        HirCicsOption::Formfeed => CicsPlanOption::Formfeed,
        HirCicsOption::DefaultScreen => CicsPlanOption::DefaultScreen,
        HirCicsOption::AlternateScreen => CicsPlanOption::AlternateScreen,
        HirCicsOption::EraseAup => CicsPlanOption::EraseAup,
        HirCicsOption::Print => CicsPlanOption::Print,
        HirCicsOption::Alarm => CicsPlanOption::Alarm,
        HirCicsOption::Frset => CicsPlanOption::Frset,
        HirCicsOption::Paging => CicsPlanOption::Paging,
        HirCicsOption::Last => CicsPlanOption::Last,
        HirCicsOption::Honeom => CicsPlanOption::Honeom,
        HirCicsOption::L40 => CicsPlanOption::L40,
        HirCicsOption::L64 => CicsPlanOption::L64,
        HirCicsOption::L80 => CicsPlanOption::L80,
        HirCicsOption::ReleasePage => CicsPlanOption::ReleasePage,
        HirCicsOption::RetainPage => CicsPlanOption::RetainPage,
        HirCicsOption::Autopage => CicsPlanOption::Autopage,
        HirCicsOption::CurrentPage => CicsPlanOption::CurrentPage,
        HirCicsOption::AllPages => CicsPlanOption::AllPages,
        HirCicsOption::NoAutopage => CicsPlanOption::NoAutopage,
        HirCicsOption::OperPurge => CicsPlanOption::OperPurge,
        HirCicsOption::NoDump => CicsPlanOption::NoDump,
        HirCicsOption::Reset => CicsPlanOption::Reset,
        HirCicsOption::Update => CicsPlanOption::Update,
        HirCicsOption::Rollback => CicsPlanOption::Rollback,
        HirCicsOption::NoHandle => CicsPlanOption::NoHandle,
        HirCicsOption::Task => CicsPlanOption::Task,
        HirCicsOption::Uow => CicsPlanOption::Uow,
        HirCicsOption::NoSuspend => CicsPlanOption::NoSuspend,
        HirCicsOption::Erase => CicsPlanOption::Erase,
        HirCicsOption::Cursor => CicsPlanOption::Cursor,
        HirCicsOption::DateSep => CicsPlanOption::DateSep,
        HirCicsOption::TimeSep => CicsPlanOption::TimeSep,
        HirCicsOption::FreeKb => CicsPlanOption::FreeKb,
        HirCicsOption::Gteq => CicsPlanOption::Gteq,
        HirCicsOption::Generic => CicsPlanOption::Generic,
        HirCicsOption::Equal => CicsPlanOption::Equal,
        HirCicsOption::Fmh => CicsPlanOption::Fmh,
        HirCicsOption::Protect => CicsPlanOption::Protect,
        HirCicsOption::Wait => CicsPlanOption::Wait,
        HirCicsOption::After => CicsPlanOption::After,
        HirCicsOption::At => CicsPlanOption::At,
        HirCicsOption::For => CicsPlanOption::For,
        HirCicsOption::Until => CicsPlanOption::Until,
        HirCicsOption::NoCheck => CicsPlanOption::NoCheck,
        HirCicsOption::MapOnly => CicsPlanOption::MapOnly,
        HirCicsOption::DataOnly => CicsPlanOption::DataOnly,
        HirCicsOption::Terminal => CicsPlanOption::Terminal,
        HirCicsOption::Purgeable => CicsPlanOption::Purgeable,
        HirCicsOption::NotPurgeable => CicsPlanOption::NotPurgeable,
        HirCicsOption::Next => CicsPlanOption::Next,
        HirCicsOption::RewriteTemporary => CicsPlanOption::RewriteTemporary,
        HirCicsOption::Auxiliary => CicsPlanOption::Auxiliary,
        HirCicsOption::Main => CicsPlanOption::Main,
        HirCicsOption::ExactMatch => CicsPlanOption::ExactMatch,
        HirCicsOption::Minimum => CicsPlanOption::Minimum,
        HirCicsOption::Hold => CicsPlanOption::Hold,
        HirCicsOption::Unescaped => CicsPlanOption::Unescaped,
        HirCicsOption::CounterNoSuspend => CicsPlanOption::CounterNoSuspend,
        HirCicsOption::CounterReduce => CicsPlanOption::CounterReduce,
        HirCicsOption::CounterWrap => CicsPlanOption::CounterWrap,
        HirCicsOption::SpoolKeep => CicsPlanOption::SpoolKeep,
        HirCicsOption::SpoolDelete => CicsPlanOption::SpoolDelete,
        HirCicsOption::SpoolNoCc => CicsPlanOption::SpoolNoCc,
        HirCicsOption::SpoolAsa => CicsPlanOption::SpoolAsa,
        HirCicsOption::SpoolMcc => CicsPlanOption::SpoolMcc,
        HirCicsOption::SpoolPrint => CicsPlanOption::SpoolPrint,
        HirCicsOption::SpoolPunch => CicsPlanOption::SpoolPunch,
        HirCicsOption::SpoolLine => CicsPlanOption::SpoolLine,
        HirCicsOption::DefResp => CicsPlanOption::DefResp,
        HirCicsOption::NoWait => CicsPlanOption::NoWait,
        HirCicsOption::Rrn => CicsPlanOption::Rrn,
        HirCicsOption::Console => CicsPlanOption::Console,
        HirCicsOption::PrintMedium => CicsPlanOption::PrintMedium,
        HirCicsOption::Card => CicsPlanOption::Card,
        HirCicsOption::WpMedia1 => CicsPlanOption::WpMedia1,
        HirCicsOption::WpMedia2 => CicsPlanOption::WpMedia2,
        HirCicsOption::WpMedia3 => CicsPlanOption::WpMedia3,
        HirCicsOption::Nleom => CicsPlanOption::Nleom,
        HirCicsOption::WpMedia4 => CicsPlanOption::WpMedia4,
        HirCicsOption::SpoolPage => CicsPlanOption::SpoolPage,
        HirCicsOption::EventAnd => CicsPlanOption::EventAnd,
        HirCicsOption::EventOr => CicsPlanOption::EventOr,
        HirCicsOption::TimerAfter => CicsPlanOption::TimerAfter,
        HirCicsOption::TimerAt => CicsPlanOption::TimerAt,
        HirCicsOption::TimerOn => CicsPlanOption::TimerOn,
        HirCicsOption::AcqActivity => CicsPlanOption::AcqActivity,
        HirCicsOption::AcqProcess => CicsPlanOption::AcqProcess,
        HirCicsOption::TraceException => CicsPlanOption::TraceException,
        HirCicsOption::DumpComplete => CicsPlanOption::DumpComplete,
        HirCicsOption::DumpTask => CicsPlanOption::DumpTask,
        HirCicsOption::DumpStorage => CicsPlanOption::DumpStorage,
        HirCicsOption::DumpProgram => CicsPlanOption::DumpProgram,
        HirCicsOption::DumpTerminal => CicsPlanOption::DumpTerminal,
        HirCicsOption::DumpTables => CicsPlanOption::DumpTables,
        HirCicsOption::DumpFct => CicsPlanOption::DumpFct,
        HirCicsOption::DumpPct => CicsPlanOption::DumpPct,
        HirCicsOption::DumpPpt => CicsPlanOption::DumpPpt,
        HirCicsOption::DumpSit => CicsPlanOption::DumpSit,
        HirCicsOption::DumpTct => CicsPlanOption::DumpTct,
        HirCicsOption::DumpTrt => CicsPlanOption::DumpTrt,
        HirCicsOption::DumpDct => CicsPlanOption::DumpDct,
        HirCicsOption::TraceOn => CicsPlanOption::TraceOn,
        HirCicsOption::TraceOff => CicsPlanOption::TraceOff,
        HirCicsOption::TraceSystem => CicsPlanOption::TraceSystem,
        HirCicsOption::TraceUser => CicsPlanOption::TraceUser,
        HirCicsOption::TraceEi => CicsPlanOption::TraceEi,
        HirCicsOption::TraceSingle => CicsPlanOption::TraceSingle,
        HirCicsOption::TraceAccount => CicsPlanOption::TraceAccount,
        HirCicsOption::TraceMonitor => CicsPlanOption::TraceMonitor,
        HirCicsOption::TracePerform => CicsPlanOption::TracePerform,
        HirCicsOption::WebBrowseHttpHeader => CicsPlanOption::WebBrowseHttpHeader,
        HirCicsOption::WebBrowseQueryParm => CicsPlanOption::WebBrowseQueryParm,
        HirCicsOption::WebBrowseFormField => CicsPlanOption::WebBrowseFormField,
        HirCicsOption::WebNotruncate => CicsPlanOption::WebNotruncate,
        HirCicsOption::IssueWaitOption => CicsPlanOption::IssueWaitOption,
        HirCicsOption::IssueEndOutput => CicsPlanOption::IssueEndOutput,
        HirCicsOption::IssueEndFile => CicsPlanOption::IssueEndFile,
        HirCicsOption::IssueConverse => CicsPlanOption::IssueConverse,
        HirCicsOption::IssueLogonLogmode => CicsPlanOption::IssueLogonLogmode,
        HirCicsOption::IssueNoQuiesce => CicsPlanOption::IssueNoQuiesce,
        HirCicsOption::WebNoClientConvert => CicsPlanOption::WebNoClientConvert,
        HirCicsOption::WebNoServerConvert => CicsPlanOption::WebNoServerConvert,
    }
}

#[cfg(test)]
mod conversation_tests {
    use super::*;
    use mainframe_env_ir::decode_cics_effect_plan;
    use std::collections::BTreeSet;

    #[test]
    fn mapped_allocate_hir_lowers_to_reserved_v2_plan() {
        let statement = HirCicsStatement {
            operation: HirCicsOperation::AllocateConversation,
            operands: vec![HirCicsNamedOperand {
                name: HirCicsOperandName::ConversationSysid,
                value: HirCicsValue::Literal("SYS1".into()),
            }],
            options: BTreeSet::from([HirCicsOption::ConversationNoQueue]),
            outputs: Vec::new(),
            condition_policy: HirCicsConditionPolicy::Default,
        };
        let encoded = encode_statement(&statement, &BTreeMap::new()).unwrap();
        assert_eq!(&encoded.bytes[..8], b"MCEP\0\x02\0\xde");
        let decoded = decode_cics_effect_plan(&encoded.bytes, CicsPlanLimits::default()).unwrap();
        assert_eq!(decoded.operation, CicsPlanOperation::AllocateConversation);
        assert!(
            decoded
                .options
                .contains(&CicsPlanOption::ConversationNoQueue)
        );
        assert!(encoded.storage.is_empty());

        let mut invalid = statement;
        invalid.operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::ConversationPartner,
            value: HirCicsValue::Literal("PARTNER1".into()),
        });
        assert!(matches!(
            encode_statement(&invalid, &BTreeMap::new()),
            Err(CicsPlanProblem::InvalidPlan)
        ));
    }
}
