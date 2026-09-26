#[cfg(test)]
use mainframe_env_host_api::CicsOperation;
use mainframe_env_ir::{
    CicsAssignOutput, CicsOperandName, CicsOutputName, CicsPlanOperation, CicsPlanOption,
};

pub(super) const fn counter_shape(operation: Option<CicsPlanOperation>) -> Option<(usize, bool)> {
    match operation {
        Some(
            CicsPlanOperation::DefineCounter
            | CicsPlanOperation::DeleteCounter
            | CicsPlanOperation::GetCounter
            | CicsPlanOperation::QueryCounter
            | CicsPlanOperation::RewindCounter
            | CicsPlanOperation::UpdateCounter,
        ) => Some((4, true)),
        Some(
            CicsPlanOperation::DefineDCounter
            | CicsPlanOperation::DeleteDCounter
            | CicsPlanOperation::GetDCounter
            | CicsPlanOperation::QueryDCounter
            | CicsPlanOperation::RewindDCounter
            | CicsPlanOperation::UpdateDCounter,
        ) => Some((8, false)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(super) enum SlotUse {
    BtsTextInput(usize),
    BtsExactInput(usize),
    BtsExactOutput(usize),
    Input,
    HalfwordInput,
    FullwordInput,
    CounterNumber,
    AbcodeInput,
    ProgramNameInput,
    AbstimeInput,
    DateStringInput,
    SeparatorInput,
    Output,
    ExactOutput(usize),
    AbstimeOutput,
    FormatTextOutput(usize),
    MillisecondsOutput,
    NumericOutput,
    HalfwordOutput,
    FullwordOutput,
    PointerInput,
    PointerOutput,
    Pointer64Output,
    Pointer64Input,
    DataArea64Input,
    AddressInput,
    AddressOutput,
    AssignOutput(CicsAssignOutput),
    SpoolTokenInput,
    SpoolTokenOutput,
    SpoolUserIdInput,
    SpoolClassInput,
    SpoolNodeInput,
    SpoolRecordLengthInput,
    SpoolOutDescrInput,
    SpoolMaxFlengthInput,
    SpoolToFlengthOutput,
    MonitorDataInput,
    DumpIdOutput,
}

pub(super) const fn input_slot_use(name: CicsOperandName) -> SlotUse {
    match name {
        CicsOperandName::BtsActivityId => SlotUse::BtsTextInput(52),
        CicsOperandName::BtsProcess => SlotUse::BtsTextInput(36),
        CicsOperandName::BtsProcessType
        | CicsOperandName::BtsProgram
        | CicsOperandName::BtsUserId => SlotUse::BtsTextInput(8),
        CicsOperandName::BtsTransId => SlotUse::BtsTextInput(4),
        CicsOperandName::BtsActivity
        | CicsOperandName::BtsEvent
        | CicsOperandName::BtsInputEvent
        | CicsOperandName::BtsLinkActivity
        | CicsOperandName::BtsLinkInputEvent
        | CicsOperandName::BtsChannel => SlotUse::BtsTextInput(16),
        CicsOperandName::ContainerName
        | CicsOperandName::ContainerAs
        | CicsOperandName::ContainerActivity
        | CicsOperandName::ContainerFromActivity
        | CicsOperandName::ContainerToActivity
        | CicsOperandName::ContainerToChannel => SlotUse::BtsTextInput(16),
        CicsOperandName::ContainerFrom64 => SlotUse::Pointer64Input,
        CicsOperandName::ContainerLength
        | CicsOperandName::ContainerCcsid
        | CicsOperandName::ContainerByteOffset
        | CicsOperandName::ContainerIntoCcsid => SlotUse::FullwordInput,
        CicsOperandName::BtsFacilityToken => SlotUse::BtsExactInput(8),
        CicsOperandName::BtsChild => SlotUse::BtsExactInput(16),
        CicsOperandName::BtsTimeout => SlotUse::FullwordInput,
        CicsOperandName::ConversationMaxProcLen => SlotUse::HalfwordInput,
        CicsOperandName::ConversationAttachId
        | CicsOperandName::ConversationConvid
        | CicsOperandName::ConversationSession
        | CicsOperandName::ConversationNetName => SlotUse::Input,
        CicsOperandName::ConversationIuType
        | CicsOperandName::ConversationDataStream
        | CicsOperandName::ConversationRecordFormat
        | CicsOperandName::ConversationProcLength
        | CicsOperandName::ConversationPipLength
        | CicsOperandName::ConversationSyncLevel
        | CicsOperandName::ConversationFromLength
        | CicsOperandName::ConversationMaxLength
        | CicsOperandName::ConversationToLength => SlotUse::HalfwordInput,
        CicsOperandName::ConversationDataLength | CicsOperandName::ConversationDataMaxLength => {
            SlotUse::HalfwordInput
        }
        CicsOperandName::ConversationFromFullLength
        | CicsOperandName::ConversationMaxFullLength
        | CicsOperandName::ConversationToFullLength => SlotUse::FullwordInput,
        CicsOperandName::ConversationDataFullLength
        | CicsOperandName::ConversationDataMaxFullLength => SlotUse::FullwordInput,
        CicsOperandName::Abcode => SlotUse::AbcodeInput,
        CicsOperandName::IssueLength => SlotUse::HalfwordInput,
        CicsOperandName::Program => SlotUse::ProgramNameInput,
        CicsOperandName::Abstime => SlotUse::AbstimeInput,
        CicsOperandName::DateString => SlotUse::DateStringInput,
        CicsOperandName::Field => SlotUse::Input,
        CicsOperandName::Record => SlotUse::Input,
        CicsOperandName::RecordLength => SlotUse::FullwordInput,
        CicsOperandName::DigestType => SlotUse::Input,
        CicsOperandName::OperatorTextLength
        | CicsOperandName::OperatorNumRoutes
        | CicsOperandName::OperatorAction
        | CicsOperandName::OperatorMaxLength
        | CicsOperandName::OperatorTimeout => SlotUse::FullwordInput,
        CicsOperandName::OperatorText
        | CicsOperandName::OperatorRouteCodes
        | CicsOperandName::OperatorConsName => SlotUse::Input,
        CicsOperandName::MajorVersion | CicsOperandName::MinorVersion => SlotUse::FullwordInput,
        CicsOperandName::DateSep | CicsOperandName::TimeSep => SlotUse::SeparatorInput,
        CicsOperandName::DestIdLength
        | CicsOperandName::Subaddress
        | CicsOperandName::VolumeLength
        | CicsOperandName::NumRec
        | CicsOperandName::KeyNumber => SlotUse::HalfwordInput,
        CicsOperandName::KeyLength => SlotUse::Input,
        CicsOperandName::Flength
        | CicsOperandName::ElementNameLength
        | CicsOperandName::ElementNamespaceLength
        | CicsOperandName::TypeNameLength
        | CicsOperandName::TypeNamespaceLength
        | CicsOperandName::JournalReqId => SlotUse::FullwordInput,
        CicsOperandName::JournalFlength => SlotUse::FullwordInput,
        CicsOperandName::SignalFromLength => SlotUse::FullwordInput,
        CicsOperandName::JournalPfxLeng => SlotUse::HalfwordInput,
        CicsOperandName::Token => SlotUse::FullwordInput,
        CicsOperandName::Flength64 => SlotUse::FullwordInput,
        CicsOperandName::ScopeLen
        | CicsOperandName::FaultCodeLen
        | CicsOperandName::FaultStrLen
        | CicsOperandName::RoleLength
        | CicsOperandName::FaultActLen
        | CicsOperandName::DetailLength
        | CicsOperandName::FromCcsid
        | CicsOperandName::SubcodeLen
        | CicsOperandName::RelatesIndex
        | CicsOperandName::EprLength
        | CicsOperandName::IntoCcsid
        | CicsOperandName::RefParmsLen
        | CicsOperandName::MetadataLen => SlotUse::FullwordInput,
        CicsOperandName::DataPointer => SlotUse::PointerInput,
        CicsOperandName::DataPointer64 => SlotUse::Pointer64Input,
        CicsOperandName::DataArea => SlotUse::Input,
        CicsOperandName::DataArea64 => SlotUse::DataArea64Input,
        CicsOperandName::EventControlAddress => SlotUse::PointerInput,
        CicsOperandName::EcbList => SlotUse::PointerInput,
        CicsOperandName::NumEvents | CicsOperandName::Purgeability => SlotUse::FullwordInput,
        CicsOperandName::Item => SlotUse::HalfwordInput,
        CicsOperandName::LoadSet | CicsOperandName::Entry => SlotUse::PointerOutput,
        CicsOperandName::LoadLength => SlotUse::HalfwordOutput,
        CicsOperandName::LoadFlength => SlotUse::FullwordOutput,
        CicsOperandName::ListLength | CicsOperandName::MaximumLength => SlotUse::FullwordInput,
        CicsOperandName::DocumentToken
        | CicsOperandName::Text
        | CicsOperandName::Binary
        | CicsOperandName::FromDocument
        | CicsOperandName::Template
        | CicsOperandName::SymbolList
        | CicsOperandName::Delimiter
        | CicsOperandName::HostCodePage
        | CicsOperandName::Bookmark
        | CicsOperandName::Symbol
        | CicsOperandName::AtBookmark
        | CicsOperandName::ToBookmark
        | CicsOperandName::CharacterSet
        | CicsOperandName::SymbolValue => SlotUse::Input,
        CicsOperandName::SpoolToken => SlotUse::SpoolTokenInput,
        CicsOperandName::SpoolUserId => SlotUse::SpoolUserIdInput,
        CicsOperandName::SpoolClass => SlotUse::SpoolClassInput,
        CicsOperandName::SpoolNode => SlotUse::SpoolNodeInput,
        CicsOperandName::SpoolRecordLength => SlotUse::SpoolRecordLengthInput,
        CicsOperandName::SpoolOutDescr => SlotUse::SpoolOutDescrInput,
        CicsOperandName::SpoolMaxFlength => SlotUse::SpoolMaxFlengthInput,
        CicsOperandName::SpoolFrom => SlotUse::Input,
        CicsOperandName::SpoolFlength => SlotUse::SpoolMaxFlengthInput,
        CicsOperandName::CounterValue
        | CicsOperandName::CounterMinimum
        | CicsOperandName::CounterMaximum
        | CicsOperandName::CounterIncrement
        | CicsOperandName::CounterCompareMin
        | CicsOperandName::CounterCompareMax => SlotUse::CounterNumber,
        CicsOperandName::TraceNum | CicsOperandName::TraceFromLength => SlotUse::HalfwordInput,
        CicsOperandName::MonitorPoint => SlotUse::HalfwordInput,
        CicsOperandName::MonitorData1 | CicsOperandName::MonitorData2 => SlotUse::MonitorDataInput,
        CicsOperandName::DumpLength => SlotUse::HalfwordInput,
        CicsOperandName::DumpFlength | CicsOperandName::DumpNumSegments => SlotUse::FullwordInput,
        CicsOperandName::WebUrlLength
        | CicsOperandName::WebHostLength
        | CicsOperandName::WebPathLength
        | CicsOperandName::WebQueryStringLength => SlotUse::FullwordInput,
        CicsOperandName::WebMethodLength
        | CicsOperandName::WebVersionLength
        | CicsOperandName::WebRealmLength => SlotUse::FullwordInput,
        CicsOperandName::WebNameLength | CicsOperandName::WebValueLength => SlotUse::FullwordInput,
        CicsOperandName::WebStatusLength | CicsOperandName::WebFromLength => SlotUse::FullwordInput,
        CicsOperandName::WebReceiveMaxLength | CicsOperandName::WebReceiveStatusLength => {
            SlotUse::FullwordInput
        }
        CicsOperandName::WebStatusCode => SlotUse::HalfwordInput,
        CicsOperandName::WebPortNumber => SlotUse::FullwordInput,
        _ => SlotUse::Input,
    }
}

pub(super) const fn output_slot_use(name: CicsOutputName) -> SlotUse {
    match name {
        CicsOutputName::BtsActivityId => SlotUse::BtsExactOutput(52),
        CicsOutputName::BtsAbCode => SlotUse::BtsExactOutput(4),
        CicsOutputName::BtsAbProgram => SlotUse::BtsExactOutput(8),
        CicsOutputName::BtsChildToken => SlotUse::BtsExactOutput(16),
        CicsOutputName::BtsCompStatus | CicsOutputName::BtsMode | CicsOutputName::BtsSuspStatus => {
            SlotUse::FullwordOutput
        }
        CicsOutputName::BtsAny | CicsOutputName::BtsChannel | CicsOutputName::BtsAbcode => {
            SlotUse::Output
        }
        CicsOutputName::BtsChildCompStatus => SlotUse::FullwordOutput,
        CicsOutputName::ContainerLength
        | CicsOutputName::ContainerCcsid
        | CicsOutputName::ContainerCount => SlotUse::FullwordOutput,
        CicsOutputName::ContainerInto | CicsOutputName::ContainerSet => SlotUse::Output,
        CicsOutputName::ContainerInto64 => SlotUse::Pointer64Input,
        CicsOutputName::AttachIuType
        | CicsOutputName::AttachDataStream
        | CicsOutputName::AttachRecordFormat
        | CicsOutputName::LogonLength
        | CicsOutputName::ProcessLength
        | CicsOutputName::SyncLevel
        | CicsOutputName::PipLength => SlotUse::HalfwordOutput,
        CicsOutputName::ConversationState => SlotUse::FullwordOutput,
        CicsOutputName::ConversationDataState => SlotUse::FullwordOutput,
        CicsOutputName::ConversationDataLength => SlotUse::HalfwordOutput,
        CicsOutputName::ConversationDataFullLength => SlotUse::FullwordOutput,
        CicsOutputName::ConversationDataSet => SlotUse::PointerOutput,
        CicsOutputName::ConversationDataInto
        | CicsOutputName::ConversationDataRetcode
        | CicsOutputName::ConversationDataConvData => SlotUse::Output,
        CicsOutputName::LogonSet | CicsOutputName::PipList => SlotUse::PointerOutput,
        CicsOutputName::AttachProcess
        | CicsOutputName::AttachResource
        | CicsOutputName::AttachReturnProcess
        | CicsOutputName::AttachReturnResource
        | CicsOutputName::AttachQueue
        | CicsOutputName::ConversationData
        | CicsOutputName::ConversationRetCode
        | CicsOutputName::LogonInto
        | CicsOutputName::ProcessName
        | CicsOutputName::TctSysId
        | CicsOutputName::TctTermId => SlotUse::Output,
        CicsOutputName::ConversationToLength => SlotUse::HalfwordOutput,
        CicsOutputName::ConversationToFullLength => SlotUse::FullwordOutput,
        CicsOutputName::ConversationSet => SlotUse::PointerOutput,
        CicsOutputName::ConversationConvid
        | CicsOutputName::ConversationRetcode
        | CicsOutputName::ConversationPrinConvid
        | CicsOutputName::ConversationPrinSysid
        | CicsOutputName::ConversationConvData
        | CicsOutputName::ConversationInto => SlotUse::Output,
        CicsOutputName::CounterValue
        | CicsOutputName::CounterMinimum
        | CicsOutputName::CounterMaximum => SlotUse::CounterNumber,
        CicsOutputName::Abstime => SlotUse::AbstimeOutput,
        CicsOutputName::TimerStatus => SlotUse::FullwordOutput,
        CicsOutputName::IssueState => SlotUse::FullwordOutput,
        CicsOutputName::IssueConvData => SlotUse::ExactOutput(24),
        CicsOutputName::IssueRetCode => SlotUse::ExactOutput(6),
        CicsOutputName::EventName | CicsOutputName::SubEventName => SlotUse::Output,
        CicsOutputName::EventType | CicsOutputName::FireStatus => SlotUse::FullwordOutput,
        CicsOutputName::Commarea
        | CicsOutputName::SecurityIsUserId
        | CicsOutputName::SecurityEncryptKey
        | CicsOutputName::SecurityPassTicket
        | CicsOutputName::SecurityLangInUse
        | CicsOutputName::SecurityNatLangInUse => SlotUse::Output,
        CicsOutputName::Field => SlotUse::Output,
        CicsOutputName::DigestResult => SlotUse::Output,
        CicsOutputName::OperatorReply => SlotUse::Output,
        CicsOutputName::OperatorReplyLength => SlotUse::FullwordOutput,
        CicsOutputName::Certificate(output) if output.pointer() => SlotUse::PointerOutput,
        CicsOutputName::Certificate(output) if output.length() => SlotUse::FullwordOutput,
        CicsOutputName::Certificate(_) => SlotUse::Output,
        CicsOutputName::Tcpip(output) if output.fullword() || output.buffer_length() => {
            SlotUse::FullwordOutput
        }
        CicsOutputName::Tcpip(_) => SlotUse::Output,
        CicsOutputName::Into => SlotUse::Output,
        CicsOutputName::Partn => SlotUse::Output,
        CicsOutputName::SetPointer => SlotUse::PointerOutput,
        CicsOutputName::SecurityOutToken | CicsOutputName::SecurityEncryptPassTicket => {
            SlotUse::PointerOutput
        }
        CicsOutputName::SetPointer64 => SlotUse::Pointer64Output,
        CicsOutputName::WebEprSet => SlotUse::PointerOutput,
        CicsOutputName::WebEprLength => SlotUse::FullwordOutput,
        CicsOutputName::WebAction
        | CicsOutputName::WebMessageId
        | CicsOutputName::WebRelatesUri
        | CicsOutputName::WebRelatesType
        | CicsOutputName::WebEprInto => SlotUse::Output,
        CicsOutputName::Ridfld => SlotUse::Output,
        CicsOutputName::Token => SlotUse::FullwordOutput,
        CicsOutputName::ReturnTransId | CicsOutputName::ReturnTermId | CicsOutputName::Queue => {
            SlotUse::Output
        }
        CicsOutputName::ElementName
        | CicsOutputName::ElementNamespace
        | CicsOutputName::TypeName
        | CicsOutputName::TypeNamespace => SlotUse::Output,
        CicsOutputName::Milliseconds => SlotUse::MillisecondsOutput,
        CicsOutputName::Mmddyy | CicsOutputName::Time | CicsOutputName::Yymmdd => {
            SlotUse::FormatTextOutput(8)
        }
        CicsOutputName::Mmddyyyy | CicsOutputName::Yyyymmdd => SlotUse::FormatTextOutput(10),
        CicsOutputName::Yyddd => SlotUse::FormatTextOutput(6),
        CicsOutputName::Resp
        | CicsOutputName::SecurityRead
        | CicsOutputName::SecurityUpdate
        | CicsOutputName::SecurityControl
        | CicsOutputName::SecurityAlter
        | CicsOutputName::SecurityChangeTime
        | CicsOutputName::SecurityDaysLeft
        | CicsOutputName::SecurityEsmReason
        | CicsOutputName::SecurityEsmResp
        | CicsOutputName::SecurityExpiryTime
        | CicsOutputName::SecurityInvalidCount
        | CicsOutputName::SecurityLastUseTime
        | CicsOutputName::SecurityOutTokenLength
        | CicsOutputName::SecurityEncryptLength
        | CicsOutputName::Resp2
        | CicsOutputName::Length
        | CicsOutputName::NumItems
        | CicsOutputName::JournalReqId => SlotUse::NumericOutput,
        CicsOutputName::ElementNameLength
        | CicsOutputName::ElementNamespaceLength
        | CicsOutputName::TypeNameLength
        | CicsOutputName::TypeNamespaceLength => SlotUse::NumericOutput,
        CicsOutputName::Assign(output) => SlotUse::AssignOutput(output),
        CicsOutputName::DocumentToken => SlotUse::Output,
        CicsOutputName::DocumentSize => SlotUse::NumericOutput,
        CicsOutputName::SpoolToken => SlotUse::SpoolTokenOutput,
        CicsOutputName::SpoolToFlength => SlotUse::SpoolToFlengthOutput,
        CicsOutputName::DumpId => SlotUse::DumpIdOutput,
        CicsOutputName::WebSchemeName
        | CicsOutputName::WebHost
        | CicsOutputName::WebPath
        | CicsOutputName::WebQueryString => SlotUse::Output,
        CicsOutputName::WebHttpMethod
        | CicsOutputName::WebHttpVersion
        | CicsOutputName::WebUriMap
        | CicsOutputName::WebRealm
        | CicsOutputName::WebValue => SlotUse::Output,
        CicsOutputName::WebBrowseName => SlotUse::Output,
        CicsOutputName::WebRetrieveDocumentToken => SlotUse::Output,
        CicsOutputName::WebReceiveInto
        | CicsOutputName::WebReceiveStatusText
        | CicsOutputName::WebReceiveMediaType
        | CicsOutputName::WebReceiveBodyCharset => SlotUse::Output,
        CicsOutputName::WebReceiveLength | CicsOutputName::WebReceiveStatusLength => {
            SlotUse::FullwordOutput
        }
        CicsOutputName::WebReceiveStatusCode => SlotUse::HalfwordOutput,
        CicsOutputName::WebConverseInto
        | CicsOutputName::WebConverseStatusText
        | CicsOutputName::WebConverseMediaType
        | CicsOutputName::WebConverseBodyCharset => SlotUse::Output,
        CicsOutputName::WebConverseToLength | CicsOutputName::WebConverseStatusLength => {
            SlotUse::FullwordOutput
        }
        CicsOutputName::WebConverseStatusCode => SlotUse::HalfwordOutput,
        CicsOutputName::WebHostLength
        | CicsOutputName::WebHostType
        | CicsOutputName::WebPortNumber
        | CicsOutputName::WebPathLength
        | CicsOutputName::WebQueryStringLength => SlotUse::FullwordOutput,
        CicsOutputName::WebScheme
        | CicsOutputName::WebMethodLength
        | CicsOutputName::WebVersionLength
        | CicsOutputName::WebRequestType
        | CicsOutputName::WebRealmLength
        | CicsOutputName::WebValueLength => SlotUse::FullwordOutput,
        CicsOutputName::WebBrowseNameLength => SlotUse::FullwordOutput,
        CicsOutputName::WebSessionToken => SlotUse::Output,
        CicsOutputName::WebHttpVNum | CicsOutputName::WebHttpRNum => SlotUse::HalfwordOutput,
    }
}

pub(super) use super::host_operation::host_operation;

pub(super) const fn operand(name: CicsOperandName) -> &'static str {
    match name {
        CicsOperandName::IssueConvid => "CONVID",
        CicsOperandName::IssueSession => "SESSION",
        CicsOperandName::IssueTermId => "TERMID",
        CicsOperandName::IssueCtlChar => "CTLCHAR",
        CicsOperandName::IssueProgram => "PROGRAM",
        CicsOperandName::IssueLuName => "LUNAME",
        CicsOperandName::IssueFrom => "FROM",
        CicsOperandName::IssueLength => "LENGTH",
        CicsOperandName::IssueLogMode => "LOGMODE",
        CicsOperandName::BtsActivityId => "ACTIVITYID",
        CicsOperandName::BtsProcess => "PROCESS",
        CicsOperandName::BtsProcessType => "PROCESSTYPE",
        CicsOperandName::BtsActivity => "ACTIVITY",
        CicsOperandName::BtsEvent => "EVENT",
        CicsOperandName::BtsInputEvent => "INPUTEVENT",
        CicsOperandName::BtsTransId => "TRANSID",
        CicsOperandName::BtsProgram => "PROGRAM",
        CicsOperandName::BtsUserId => "USERID",
        CicsOperandName::BtsFacilityToken => "FACILITYTOKN",
        CicsOperandName::BtsChannel => "CHANNEL",
        CicsOperandName::ContainerName => "CONTAINER",
        CicsOperandName::ContainerAs => "AS",
        CicsOperandName::ContainerActivity => "ACTIVITY",
        CicsOperandName::ContainerFromActivity => "FROMACTIVITY",
        CicsOperandName::ContainerToActivity => "TOACTIVITY",
        CicsOperandName::ContainerToChannel => "TOCHANNEL",
        CicsOperandName::ContainerFrom => "FROM",
        CicsOperandName::ContainerFrom64 => "FROM",
        CicsOperandName::ContainerLength => "FLENGTH",
        CicsOperandName::ContainerDatatype => "DATATYPE",
        CicsOperandName::ContainerCcsid => "FROMCCSID",
        CicsOperandName::ContainerByteOffset => "BYTEOFFSET",
        CicsOperandName::ContainerIntoCcsid => "INTOCCSID",
        CicsOperandName::ContainerFromCodepage => "FROMCODEPAGE",
        CicsOperandName::ContainerIntoCodepage => "INTOCODEPAGE",
        CicsOperandName::ContainerConvertst => "CONVERTST",
        CicsOperandName::BtsChild => "CHILD",
        CicsOperandName::BtsLinkActivity => "ACTIVITY",
        CicsOperandName::BtsLinkInputEvent => "INPUTEVENT",
        CicsOperandName::BtsTimeout => "TIMEOUT",
        CicsOperandName::ConversationAttachId => "ATTACHID",
        CicsOperandName::ConversationConvid => "CONVID",
        CicsOperandName::ConversationSession => "SESSION",
        CicsOperandName::ConversationMaxProcLen => "MAXPROCLEN",
        CicsOperandName::ConversationNetName => "NETNAME",
        CicsOperandName::ConversationSysid => "SYSID",
        CicsOperandName::ConversationPartner => "PARTNER",
        CicsOperandName::ConversationProfile => "PROFILE",
        CicsOperandName::ConversationModeName => "MODENAME",
        CicsOperandName::ConversationProcess => "PROCESS",
        CicsOperandName::ConversationResource => "RESOURCE",
        CicsOperandName::ConversationReturnProcess => "RPROCESS",
        CicsOperandName::ConversationReturnResource => "RRESOURCE",
        CicsOperandName::ConversationQueue => "QUEUE",
        CicsOperandName::ConversationIuType => "IUTYPE",
        CicsOperandName::ConversationDataStream => "DATASTR",
        CicsOperandName::ConversationRecordFormat => "RECFM",
        CicsOperandName::ConversationProcName => "PROCNAME",
        CicsOperandName::ConversationProcLength => "PROCLENGTH",
        CicsOperandName::ConversationPipList => "PIPLIST",
        CicsOperandName::ConversationPipLength => "PIPLENGTH",
        CicsOperandName::ConversationSyncLevel => "SYNCLEVEL",
        CicsOperandName::ConversationFrom => "FROM",
        CicsOperandName::ConversationFromLength => "FROMLENGTH",
        CicsOperandName::ConversationFromFullLength => "FROMFLENGTH",
        CicsOperandName::ConversationMaxLength => "MAXLENGTH",
        CicsOperandName::ConversationMaxFullLength => "MAXFLENGTH",
        CicsOperandName::ConversationToLength => "TOLENGTH",
        CicsOperandName::ConversationToFullLength => "TOFLENGTH",
        CicsOperandName::ConversationDataConvid => "CONVID",
        CicsOperandName::ConversationDataSession => "SESSION",
        CicsOperandName::ConversationDataFrom => "FROM",
        CicsOperandName::ConversationDataLength => "LENGTH",
        CicsOperandName::ConversationDataFullLength => "FLENGTH",
        CicsOperandName::ConversationDataMaxLength => "MAXLENGTH",
        CicsOperandName::ConversationDataMaxFullLength => "MAXFLENGTH",
        CicsOperandName::ConversationDataAttachId => "ATTACHID",
        CicsOperandName::ResClass => "RESCLASS",
        CicsOperandName::ResId => "RESID",
        CicsOperandName::ResIdLength => "RESIDLENGTH",
        CicsOperandName::ResType => "RESTYPE",
        CicsOperandName::LogMessage => "LOGMESSAGE",
        CicsOperandName::SecurityUserId => "USERID",
        CicsOperandName::SecurityGroupId => "GROUPID",
        CicsOperandName::SecurityPassword => "PASSWORD",
        CicsOperandName::SecurityNewPassword => "NEWPASSWORD",
        CicsOperandName::SecurityNewPhrase => "NEWPHRASE",
        CicsOperandName::SecurityNewPhraseLen => "NEWPHRASELEN",
        CicsOperandName::SecurityEsmAppName => "ESMAPPNAME",
        CicsOperandName::SecurityTokenData => "TOKEN",
        CicsOperandName::SecurityTokenLength => "TOKENLEN",
        CicsOperandName::SecurityEncryptKey => "ENCRYPTKEY",
        CicsOperandName::SecurityLanguageCode => "LANGUAGECODE",
        CicsOperandName::SecurityNatLang => "NATLANG",
        CicsOperandName::SecurityOidCard => "OIDCARD",
        CicsOperandName::SecurityPhrase => "PHRASE",
        CicsOperandName::SecurityPhraseLen => "PHRASELEN",
        CicsOperandName::Abcode => "ABCODE",
        CicsOperandName::Event => "EVENT",
        CicsOperandName::SubEvent => "SUBEVENT",
        CicsOperandName::SubEvent1 => "SUBEVENT1",
        CicsOperandName::SubEvent2 => "SUBEVENT2",
        CicsOperandName::SubEvent3 => "SUBEVENT3",
        CicsOperandName::SubEvent4 => "SUBEVENT4",
        CicsOperandName::SubEvent5 => "SUBEVENT5",
        CicsOperandName::SubEvent6 => "SUBEVENT6",
        CicsOperandName::SubEvent7 => "SUBEVENT7",
        CicsOperandName::SubEvent8 => "SUBEVENT8",
        CicsOperandName::SignalFrom => "FROM",
        CicsOperandName::SignalFromLength => "FROMLENGTH",
        CicsOperandName::SignalFromChannel => "FROMCHANNEL",
        CicsOperandName::Timer => "TIMER",
        CicsOperandName::TimerDays => "DAYS",
        CicsOperandName::TimerHours => "HOURS",
        CicsOperandName::TimerMinutes => "MINUTES",
        CicsOperandName::TimerSeconds => "SECONDS",
        CicsOperandName::TimerYear => "YEAR",
        CicsOperandName::TimerMonth => "MONTH",
        CicsOperandName::TimerDayOfMonth => "DAYOFMONTH",
        CicsOperandName::TimerDayOfYear => "DAYOFYEAR",
        CicsOperandName::Label => "LABEL",
        CicsOperandName::Program => "PROGRAM",
        CicsOperandName::Commarea => "COMMAREA",
        CicsOperandName::TransId => "TRANSID",
        CicsOperandName::BrExit => "BREXIT",
        CicsOperandName::BrData => "BRDATA",
        CicsOperandName::BrDataLength => "BRDATALENGTH",
        CicsOperandName::TermId => "TERMID",
        CicsOperandName::ReturnTransId => "RTRANSID",
        CicsOperandName::ReturnTermId => "RTERMID",
        CicsOperandName::UserId => "USERID",
        CicsOperandName::File => "FILE",
        CicsOperandName::Dataset => "DATASET",
        CicsOperandName::From => "FROM",
        CicsOperandName::Ridfld => "RIDFLD",
        CicsOperandName::Token => "TOKEN",
        CicsOperandName::Queue => "QUEUE",
        CicsOperandName::Qname => "QNAME",
        CicsOperandName::SysId => "SYSID",
        CicsOperandName::CommareaPointer => "COMMAREA",
        CicsOperandName::Map => "MAP",
        CicsOperandName::Mapset => "MAPSET",
        CicsOperandName::DestId => "DESTID",
        CicsOperandName::DestIdLength => "DESTIDLENG",
        CicsOperandName::Subaddress => "SUBADDR",
        CicsOperandName::Volume => "VOLUME",
        CicsOperandName::VolumeLength => "VOLUMELENG",
        CicsOperandName::NumRec => "NUMREC",
        CicsOperandName::Errterm => "ERRTERM",
        CicsOperandName::RouteTitle => "TITLE",
        CicsOperandName::RouteList => "LIST",
        CicsOperandName::Opclass => "OPCLASS",
        CicsOperandName::KeyNumber => "KEYNUMBER",
        CicsOperandName::Partnset => "PARTNSET",
        CicsOperandName::ControlCursor => "CURSOR",
        CicsOperandName::Msr => "MSR",
        CicsOperandName::Outpartn => "OUTPARTN",
        CicsOperandName::Actpartn => "ACTPARTN",
        CicsOperandName::Ldc => "LDC",
        CicsOperandName::Trailer => "TRAILER",
        CicsOperandName::Fmhparm => "FMHPARM",
        CicsOperandName::Resource => "RESOURCE",
        CicsOperandName::Length => "LENGTH",
        CicsOperandName::DataLength => "DATALENGTH",
        CicsOperandName::MaxLifetime => "MAXLIFETIME",
        CicsOperandName::Priority => "PRIORITY",
        CicsOperandName::UserCorrData => "USERCORRDATA",
        CicsOperandName::SetAddress => "SET.ADDRESS",
        CicsOperandName::SetPointer => "SET.POINTER",
        CicsOperandName::UsingAddress => "USING.ADDRESS",
        CicsOperandName::UsingPointer => "USING.POINTER",
        CicsOperandName::Conditions => "CONDITIONS",
        CicsOperandName::Aids => "AIDS",
        CicsOperandName::Abstime => "ABSTIME",
        CicsOperandName::DateString => "DATESTRING",
        CicsOperandName::Field => "FIELD",
        CicsOperandName::Record => "RECORD",
        CicsOperandName::RecordLength => "RECORDLEN",
        CicsOperandName::DigestType => "DIGESTTYPE",
        CicsOperandName::OperatorText => "TEXT",
        CicsOperandName::OperatorTextLength => "TEXTLENGTH",
        CicsOperandName::OperatorRouteCodes => "ROUTECODES",
        CicsOperandName::OperatorNumRoutes => "NUMROUTES",
        CicsOperandName::OperatorConsName => "CONSNAME",
        CicsOperandName::OperatorAction => "ACTION",
        CicsOperandName::OperatorMaxLength => "MAXLENGTH",
        CicsOperandName::OperatorTimeout => "TIMEOUT",
        CicsOperandName::DateSep => "DATESEP",
        CicsOperandName::TimeSep => "TIMESEP",
        CicsOperandName::KeyLength => "KEYLENGTH",
        CicsOperandName::ReqId => "REQID",
        CicsOperandName::Interval => "INTERVAL",
        CicsOperandName::StartTime => "TIME",
        CicsOperandName::Hours => "HOURS",
        CicsOperandName::Minutes => "MINUTES",
        CicsOperandName::Seconds => "SECONDS",
        CicsOperandName::Milliseconds => "MILLISECS",
        CicsOperandName::Flength => "FLENGTH",
        CicsOperandName::Flength64 => "FLENGTH",
        CicsOperandName::Location64 => "LOCATION",
        CicsOperandName::Abi64 => "ABI64",
        CicsOperandName::InitImage => "INITIMG",
        CicsOperandName::DataPointer => "DATAPOINTER",
        CicsOperandName::DataPointer64 => "DATAPOINTER",
        CicsOperandName::DataArea => "DATA",
        CicsOperandName::DataArea64 => "DATA",
        CicsOperandName::EventControlAddress => "ECADDR",
        CicsOperandName::WaitName => "NAME",
        CicsOperandName::EcbList => "ECBLIST",
        CicsOperandName::NumEvents => "NUMEVENTS",
        CicsOperandName::Purgeability => "PURGEABILITY",
        CicsOperandName::Item => "ITEM",
        CicsOperandName::Application => "APPLICATION",
        CicsOperandName::Platform => "PLATFORM",
        CicsOperandName::ApplicationOperation => "OPERATION",
        CicsOperandName::MajorVersion => "MAJORVERSION",
        CicsOperandName::MinorVersion => "MINORVERSION",
        CicsOperandName::Channel => "CHANNEL",
        CicsOperandName::LoadSet => "SET",
        CicsOperandName::Entry => "ENTRY",
        CicsOperandName::LoadLength => "LENGTH",
        CicsOperandName::LoadFlength => "FLENGTH",
        CicsOperandName::DocumentToken => "DOCTOKEN",
        CicsOperandName::Text => "TEXT",
        CicsOperandName::Binary => "BINARY",
        CicsOperandName::FromDocument => "FROMDOC",
        CicsOperandName::Template => "TEMPLATE",
        CicsOperandName::SymbolList => "SYMBOLLIST",
        CicsOperandName::ListLength => "LISTLENGTH",
        CicsOperandName::Delimiter => "DELIMITER",
        CicsOperandName::HostCodePage => "HOSTCODEPAGE",
        CicsOperandName::Bookmark => "BOOKMARK",
        CicsOperandName::Symbol => "SYMBOL",
        CicsOperandName::AtBookmark => "AT",
        CicsOperandName::ToBookmark => "TO",
        CicsOperandName::MaximumLength => "MAXLENGTH",
        CicsOperandName::CharacterSet => "CHARACTERSET",
        CicsOperandName::SymbolValue => "VALUE",
        CicsOperandName::Service => "SERVICE",
        CicsOperandName::ServiceOperation => "OPERATION",
        CicsOperandName::Uri => "URI",
        CicsOperandName::UriMap => "URIMAP",
        CicsOperandName::Scope => "SCOPE",
        CicsOperandName::ScopeLen => "SCOPELEN",
        CicsOperandName::FaultCode => "FAULTCODE",
        CicsOperandName::FaultCodeStr => "FAULTCODESTR",
        CicsOperandName::FaultCodeLen => "FAULTCODELEN",
        CicsOperandName::FaultString => "FAULTSTRING",
        CicsOperandName::FaultStrLen => "FAULTSTRLEN",
        CicsOperandName::NatLang => "NATLANG",
        CicsOperandName::SoapRole => "ROLE",
        CicsOperandName::RoleLength => "ROLELENGTH",
        CicsOperandName::FaultActor => "FAULTACTOR",
        CicsOperandName::FaultActLen => "FAULTACTLEN",
        CicsOperandName::Detail => "DETAIL",
        CicsOperandName::DetailLength => "DETAILLENGTH",
        CicsOperandName::FromCcsid => "FROMCCSID",
        CicsOperandName::SubcodeStr => "SUBCODESTR",
        CicsOperandName::SubcodeLen => "SUBCODELEN",
        CicsOperandName::ContextType => "CONTEXTTYPE",
        CicsOperandName::Action => "ACTION",
        CicsOperandName::MessageId => "MESSAGEID",
        CicsOperandName::RelatesUri => "RELATESURI",
        CicsOperandName::RelatesType => "RELATESTYPE",
        CicsOperandName::RelatesIndex => "RELATESINDEX",
        CicsOperandName::EprType => "EPRTYPE",
        CicsOperandName::EprField => "EPRFIELD",
        CicsOperandName::EprFrom => "EPRFROM",
        CicsOperandName::EprLength => "EPRLENGTH",
        CicsOperandName::FromCodepage => "FROMCODEPAGE",
        CicsOperandName::IntoCcsid => "INTOCCSID",
        CicsOperandName::IntoCodepage => "INTOCODEPAGE",
        CicsOperandName::Address => "ADDRESS",
        CicsOperandName::RefParms => "REFPARMS",
        CicsOperandName::RefParmsLen => "REFPARMSLEN",
        CicsOperandName::Metadata => "METADATA",
        CicsOperandName::MetadataLen => "METADATALEN",
        CicsOperandName::InContainer => "INCONTAINER",
        CicsOperandName::OutContainer => "OUTCONTAINER",
        CicsOperandName::Transformer => "TRANSFORMER",
        CicsOperandName::DataContainer => "DATCONTAINER",
        CicsOperandName::XmlContainer => "XMLCONTAINER",
        CicsOperandName::XmlTransform => "XMLTRANSFORM",
        CicsOperandName::NsContainer => "NSCONTAINER",
        CicsOperandName::ElementName => "ELEMNAME",
        CicsOperandName::ElementNameLength => "ELEMNAMELEN",
        CicsOperandName::ElementNamespace => "ELEMNS",
        CicsOperandName::ElementNamespaceLength => "ELEMNSLEN",
        CicsOperandName::TypeName => "TYPENAME",
        CicsOperandName::TypeNameLength => "TYPENAMELEN",
        CicsOperandName::TypeNamespace => "TYPENS",
        CicsOperandName::TypeNamespaceLength => "TYPENSLEN",
        CicsOperandName::JournalName => "JOURNALNAME",
        CicsOperandName::JournalNum => "JOURNALNUM",
        CicsOperandName::JournalReqId => "REQID",
        CicsOperandName::JournalTypeId => "JTYPEID",
        CicsOperandName::JournalFrom => "FROM",
        CicsOperandName::JournalFlength => "FLENGTH",
        CicsOperandName::JournalPrefix => "PREFIX",
        CicsOperandName::JournalPfxLeng => "PFXLENG",
        CicsOperandName::SpoolToken => "TOKEN",
        CicsOperandName::SpoolUserId => "USERID",
        CicsOperandName::SpoolClass => "CLASS",
        CicsOperandName::SpoolNode => "NODE",
        CicsOperandName::SpoolRecordLength => "RECORDLENGTH",
        CicsOperandName::SpoolOutDescr => "OUTDESCR",
        CicsOperandName::SpoolMaxFlength => "MAXFLENGTH",
        CicsOperandName::SpoolFrom => "FROM",
        CicsOperandName::SpoolFlength => "FLENGTH",
        CicsOperandName::CounterName => "COUNTER",
        CicsOperandName::CounterPool => "POOL",
        CicsOperandName::CounterValue => "VALUE",
        CicsOperandName::CounterMinimum => "MINIMUM",
        CicsOperandName::CounterMaximum => "MAXIMUM",
        CicsOperandName::CounterIncrement => "INCREMENT",
        CicsOperandName::CounterCompareMin => "COMPAREMIN",
        CicsOperandName::CounterCompareMax => "COMPAREMAX",
        CicsOperandName::TraceNum => "TRACENUM",
        CicsOperandName::TraceFrom => "FROM",
        CicsOperandName::TraceFromLength => "FROMLENGTH",
        CicsOperandName::TraceResource => "RESOURCE",
        CicsOperandName::MonitorPoint => "POINT",
        CicsOperandName::MonitorEntryName => "ENTRYNAME",
        CicsOperandName::MonitorData1 => "DATA1",
        CicsOperandName::MonitorData2 => "DATA2",
        CicsOperandName::DumpCode => "DUMPCODE",
        CicsOperandName::DumpFrom => "FROM",
        CicsOperandName::DumpLength => "LENGTH",
        CicsOperandName::DumpFlength => "FLENGTH",
        CicsOperandName::DumpSegmentList => "SEGMENTLIST",
        CicsOperandName::DumpLengthList => "LENGTHLIST",
        CicsOperandName::DumpNumSegments => "NUMSEGMENTS",
        CicsOperandName::TraceId => "TRACEID",
        CicsOperandName::TraceIdFrom => "FROM",
        CicsOperandName::TraceIdResource => "RESOURCE",
        CicsOperandName::TraceEntryName => "ENTRYNAME",
        CicsOperandName::WebUrl => "URL",
        CicsOperandName::WebUrlLength => "URLLENGTH",
        CicsOperandName::WebHostLength => "HOSTLENGTH",
        CicsOperandName::WebPathLength => "PATHLENGTH",
        CicsOperandName::WebQueryStringLength => "QUERYSTRLEN",
        CicsOperandName::WebHost => "HOST",
        CicsOperandName::WebSessionToken => "SESSTOKEN",
        CicsOperandName::WebMethodLength => "METHODLENGTH",
        CicsOperandName::WebVersionLength => "VERSIONLEN",
        CicsOperandName::WebRealmLength => "REALMLEN",
        CicsOperandName::WebHttpHeaderName => "HTTPHEADER",
        CicsOperandName::WebQueryParmName => "QUERYPARM",
        CicsOperandName::WebFormFieldName => "FORMFIELD",
        CicsOperandName::WebNameLength => "NAMELENGTH",
        CicsOperandName::WebValueLength => "VALUELENGTH",
        CicsOperandName::WebBrowseStartName => "BROWSESTARTNAME",
        CicsOperandName::WebHeaderValue => "VALUE",
        CicsOperandName::WebMethod => "METHOD",
        CicsOperandName::WebAction => "ACTION",
        CicsOperandName::WebCloseStatus => "CLOSESTATUS",
        CicsOperandName::WebDocumentToken => "DOCTOKEN",
        CicsOperandName::WebStatusCode => "STATUSCODE",
        CicsOperandName::WebStatusText => "STATUSTEXT",
        CicsOperandName::WebStatusLength => "STATUSLEN",
        CicsOperandName::WebFrom => "FROM",
        CicsOperandName::WebFromLength => "FROMLENGTH",
        CicsOperandName::WebPathInput => "PATH",
        CicsOperandName::WebQueryInput => "QUERYSTRING",
        CicsOperandName::WebMediaType => "MEDIATYPE",
        CicsOperandName::WebSendUriMap => "URIMAP",
        CicsOperandName::WebReceiveMaxLength => "MAXLENGTH",
        CicsOperandName::WebReceiveStatusLength => "STATUSLEN",
        CicsOperandName::WebPortNumber => "PORTNUMBER",
        CicsOperandName::WebScheme => "SCHEME",
        CicsOperandName::WebUriMap => "URIMAP",
        CicsOperandName::WebCertificate => "CERTIFICATE",
        CicsOperandName::WebCodePage => "CODEPAGE",
    }
}

pub(super) const fn operand_for(
    operation: CicsPlanOperation,
    name: CicsOperandName,
) -> &'static str {
    if matches!(
        operation,
        CicsPlanOperation::DefineDCounter
            | CicsPlanOperation::DeleteDCounter
            | CicsPlanOperation::GetDCounter
            | CicsPlanOperation::QueryDCounter
            | CicsPlanOperation::RewindDCounter
            | CicsPlanOperation::UpdateDCounter
    ) && matches!(name, CicsOperandName::CounterName)
    {
        "DCOUNTER"
    } else {
        operand(name)
    }
}

pub(super) const fn output(name: CicsOutputName) -> &'static str {
    match name {
        CicsOutputName::IssueState => "STATE",
        CicsOutputName::IssueConvData => "CONVDATA",
        CicsOutputName::IssueRetCode => "RETCODE",
        CicsOutputName::BtsActivityId => "ACTIVITYID",
        CicsOutputName::BtsCompStatus => "COMPSTATUS",
        CicsOutputName::BtsMode => "MODE",
        CicsOutputName::BtsSuspStatus => "SUSPSTATUS",
        CicsOutputName::BtsAbCode => "ABCODE",
        CicsOutputName::BtsAbProgram => "ABPROGRAM",
        CicsOutputName::BtsChildToken => "CHILD",
        CicsOutputName::BtsAny => "ANY",
        CicsOutputName::BtsChildCompStatus => "COMPSTATUS",
        CicsOutputName::BtsChannel => "CHANNEL",
        CicsOutputName::BtsAbcode => "ABCODE",
        CicsOutputName::ContainerInto => "INTO",
        CicsOutputName::ContainerInto64 => "INTO",
        CicsOutputName::ContainerSet => "SET",
        CicsOutputName::ContainerLength => "FLENGTH",
        CicsOutputName::ContainerCcsid => "CCSID",
        CicsOutputName::ContainerCount => "CONTAINERCNT",
        CicsOutputName::AttachProcess => "PROCESS",
        CicsOutputName::AttachResource => "RESOURCE",
        CicsOutputName::AttachReturnProcess => "RPROCESS",
        CicsOutputName::AttachReturnResource => "RRESOURCE",
        CicsOutputName::AttachQueue => "QUEUE",
        CicsOutputName::AttachIuType => "IUTYPE",
        CicsOutputName::AttachDataStream => "DATASTR",
        CicsOutputName::AttachRecordFormat => "RECFM",
        CicsOutputName::ConversationState => "STATE",
        CicsOutputName::ConversationData => "CONVDATA",
        CicsOutputName::ConversationRetCode => "RETCODE",
        CicsOutputName::LogonInto => "INTO",
        CicsOutputName::LogonSet => "SET",
        CicsOutputName::LogonLength => "LENGTH",
        CicsOutputName::ProcessName => "PROCNAME",
        CicsOutputName::ProcessLength => "PROCLENGTH",
        CicsOutputName::SyncLevel => "SYNCLEVEL",
        CicsOutputName::PipList => "PIPLIST",
        CicsOutputName::PipLength => "PIPLENGTH",
        CicsOutputName::TctSysId => "SYSID",
        CicsOutputName::TctTermId => "TERMID",
        CicsOutputName::ConversationConvid => "CONVID",
        CicsOutputName::ConversationRetcode => "RETCODE",
        CicsOutputName::ConversationPrinConvid => "PRINCONVID",
        CicsOutputName::ConversationPrinSysid => "PRINSYSID",
        CicsOutputName::ConversationConvData => "CONVDATA",
        CicsOutputName::ConversationInto => "INTO",
        CicsOutputName::ConversationSet => "SET",
        CicsOutputName::ConversationToLength => "TOLENGTH",
        CicsOutputName::ConversationToFullLength => "TOFLENGTH",
        CicsOutputName::ConversationDataInto => "INTO",
        CicsOutputName::ConversationDataSet => "SET",
        CicsOutputName::ConversationDataLength => "LENGTH",
        CicsOutputName::ConversationDataFullLength => "FLENGTH",
        CicsOutputName::ConversationDataRetcode => "RETCODE",
        CicsOutputName::ConversationDataConvData => "CONVDATA",
        CicsOutputName::ConversationDataState => "STATE",
        CicsOutputName::CounterValue => "VALUE",
        CicsOutputName::CounterMinimum => "MINIMUM",
        CicsOutputName::CounterMaximum => "MAXIMUM",
        CicsOutputName::Abstime => "ABSTIME",
        CicsOutputName::SecurityRead => "READ",
        CicsOutputName::SecurityUpdate => "UPDATE",
        CicsOutputName::SecurityControl => "CONTROL",
        CicsOutputName::SecurityAlter => "ALTER",
        CicsOutputName::SecurityChangeTime => "CHANGETIME",
        CicsOutputName::SecurityDaysLeft => "DAYSLEFT",
        CicsOutputName::SecurityEsmReason => "ESMREASON",
        CicsOutputName::SecurityEsmResp => "ESMRESP",
        CicsOutputName::SecurityExpiryTime => "EXPIRYTIME",
        CicsOutputName::SecurityInvalidCount => "INVALIDCOUNT",
        CicsOutputName::SecurityLastUseTime => "LASTUSETIME",
        CicsOutputName::SecurityPassTicket => "PASSTICKET",
        CicsOutputName::SecurityIsUserId => "ISUSERID",
        CicsOutputName::SecurityEncryptKey => "ENCRYPTKEY",
        CicsOutputName::SecurityOutToken => "OUTTOKEN",
        CicsOutputName::SecurityOutTokenLength => "OUTTOKENLEN",
        CicsOutputName::SecurityEncryptPassTicket => "ENCRYPTPTKT",
        CicsOutputName::SecurityEncryptLength => "FLENGTH",
        CicsOutputName::SecurityLangInUse => "LANGINUSE",
        CicsOutputName::SecurityNatLangInUse => "NATLANGINUSE",
        CicsOutputName::TimerStatus => "STATUS",
        CicsOutputName::EventName => "EVENT",
        CicsOutputName::SubEventName => "SUBEVENT",
        CicsOutputName::EventType => "EVENTTYPE",
        CicsOutputName::FireStatus => "FIRESTATUS",
        CicsOutputName::Field => "FIELD",
        CicsOutputName::DigestResult => "RESULT",
        CicsOutputName::OperatorReply => "REPLY",
        CicsOutputName::OperatorReplyLength => "REPLYLENGTH",
        CicsOutputName::Certificate(output) => output.name(),
        CicsOutputName::Tcpip(output) => output.name(),
        CicsOutputName::Commarea => "COMMAREA",
        CicsOutputName::Into => "INTO",
        CicsOutputName::Partn => "PARTN",
        CicsOutputName::SetPointer => "SET",
        CicsOutputName::SetPointer64 => "SET64",
        CicsOutputName::Ridfld => "RIDFLD",
        CicsOutputName::Token => "TOKEN",
        CicsOutputName::Milliseconds => "MILLISECONDS",
        CicsOutputName::Mmddyy => "MMDDYY",
        CicsOutputName::Mmddyyyy => "MMDDYYYY",
        CicsOutputName::Resp => "RESP",
        CicsOutputName::Resp2 => "RESP2",
        CicsOutputName::Time => "TIME",
        CicsOutputName::Yyddd => "YYDDD",
        CicsOutputName::Yymmdd => "YYMMDD",
        CicsOutputName::Yyyymmdd => "YYYYMMDD",
        CicsOutputName::Assign(output) => output.name(),
        CicsOutputName::Length => "LENGTH",
        CicsOutputName::ReturnTransId => "RTRANSID",
        CicsOutputName::ReturnTermId => "RTERMID",
        CicsOutputName::Queue => "QUEUE",
        CicsOutputName::NumItems => "NUMITEMS",
        CicsOutputName::DocumentToken => "DOCTOKEN",
        CicsOutputName::DocumentSize => "DOCSIZE",
        CicsOutputName::WebAction => "ACTION",
        CicsOutputName::WebMessageId => "MESSAGEID",
        CicsOutputName::WebRelatesUri => "RELATESURI",
        CicsOutputName::WebRelatesType => "RELATESTYPE",
        CicsOutputName::WebEprInto => "EPRINTO",
        CicsOutputName::WebEprSet => "EPRSET",
        CicsOutputName::WebEprLength => "EPRLENGTH",
        CicsOutputName::ElementName => "ELEMNAME",
        CicsOutputName::ElementNameLength => "ELEMNAMELEN",
        CicsOutputName::ElementNamespace => "ELEMNS",
        CicsOutputName::ElementNamespaceLength => "ELEMNSLEN",
        CicsOutputName::TypeName => "TYPENAME",
        CicsOutputName::TypeNameLength => "TYPENAMELEN",
        CicsOutputName::TypeNamespace => "TYPENS",
        CicsOutputName::TypeNamespaceLength => "TYPENSLEN",
        CicsOutputName::JournalReqId => "REQID",
        CicsOutputName::SpoolToken => "TOKEN",
        CicsOutputName::SpoolToFlength => "TOFLENGTH",
        CicsOutputName::DumpId => "DUMPID",
        CicsOutputName::WebSchemeName => "SCHEMENAME",
        CicsOutputName::WebHost => "HOST",
        CicsOutputName::WebHostLength => "HOSTLENGTH",
        CicsOutputName::WebHostType => "HOSTTYPE",
        CicsOutputName::WebPortNumber => "PORTNUMBER",
        CicsOutputName::WebPath => "PATH",
        CicsOutputName::WebPathLength => "PATHLENGTH",
        CicsOutputName::WebQueryString => "QUERYSTRING",
        CicsOutputName::WebQueryStringLength => "QUERYSTRLEN",
        CicsOutputName::WebSessionToken => "SESSTOKEN",
        CicsOutputName::WebHttpVNum => "HTTPVNUM",
        CicsOutputName::WebHttpRNum => "HTTPRNUM",
        CicsOutputName::WebScheme => "SCHEME",
        CicsOutputName::WebHttpMethod => "HTTPMETHOD",
        CicsOutputName::WebMethodLength => "METHODLENGTH",
        CicsOutputName::WebHttpVersion => "HTTPVERSION",
        CicsOutputName::WebVersionLength => "VERSIONLEN",
        CicsOutputName::WebRequestType => "REQUESTTYPE",
        CicsOutputName::WebUriMap => "URIMAP",
        CicsOutputName::WebRealm => "REALM",
        CicsOutputName::WebRealmLength => "REALMLEN",
        CicsOutputName::WebValue => "VALUE",
        CicsOutputName::WebValueLength => "VALUELENGTH",
        CicsOutputName::WebBrowseName => "BROWSENAME",
        CicsOutputName::WebBrowseNameLength => "NAMELENGTH",
        CicsOutputName::WebRetrieveDocumentToken => "DOCTOKEN",
        CicsOutputName::WebReceiveInto => "INTO",
        CicsOutputName::WebReceiveLength => "LENGTH",
        CicsOutputName::WebReceiveStatusCode => "STATUSCODE",
        CicsOutputName::WebReceiveStatusText => "STATUSTEXT",
        CicsOutputName::WebReceiveStatusLength => "STATUSLEN",
        CicsOutputName::WebReceiveMediaType => "MEDIATYPE",
        CicsOutputName::WebReceiveBodyCharset => "BODYCHARSET",
        CicsOutputName::WebConverseInto => "INTO",
        CicsOutputName::WebConverseToLength => "TOLENGTH",
        CicsOutputName::WebConverseStatusCode => "STATUSCODE",
        CicsOutputName::WebConverseStatusText => "STATUSTEXT",
        CicsOutputName::WebConverseStatusLength => "STATUSLEN",
        CicsOutputName::WebConverseMediaType => "MEDIATYPE",
        CicsOutputName::WebConverseBodyCharset => "BODYCHARSET",
    }
}

pub(super) const fn option(option: CicsPlanOption) -> &'static str {
    match option {
        CicsPlanOption::BtsSynchronous => "SYNCHRONOUS",
        CicsPlanOption::BtsAsynchronous => "ASYNCHRONOUS",
        CicsPlanOption::BtsNoSuspend => "NOSUSPEND",
        CicsPlanOption::BtsAcqActivity => "ACQACTIVITY",
        CicsPlanOption::BtsAcqProcess => "ACQPROCESS",
        CicsPlanOption::ContainerAppend => "APPEND",
        CicsPlanOption::ContainerNoData => "NODATA",
        CicsPlanOption::ContainerProcess => "PROCESS",
        CicsPlanOption::ContainerAcqProcess => "ACQPROCESS",
        CicsPlanOption::ContainerAcqActivity => "ACQACTIVITY",
        CicsPlanOption::ContainerFromProcess => "FROMPROCESS",
        CicsPlanOption::ContainerToProcess => "TOPROCESS",
        CicsPlanOption::ConversationNoQueue => "NOQUEUE",
        CicsPlanOption::ConversationNotruncate => "NOTRUNCATE",
        CicsPlanOption::ConversationDefresp => "DEFRESP",
        CicsPlanOption::ConversationFmh => "FMH",
        CicsPlanOption::ConversationDataNotruncate => "NOTRUNCATE",
        CicsPlanOption::ConversationDataBuffer => "BUFFER",
        CicsPlanOption::ConversationDataLlid => "LLID",
        CicsPlanOption::ConversationDataInvite => "INVITE",
        CicsPlanOption::ConversationDataLast => "LAST",
        CicsPlanOption::ConversationDataConfirm => "CONFIRM",
        CicsPlanOption::ConversationDataWait => "WAIT",
        CicsPlanOption::ConversationDataFmh => "FMH",
        CicsPlanOption::ConversationDataDefresp => "DEFRESP",
        CicsPlanOption::DefResp => "DEFRESP",
        CicsPlanOption::NoWait => "NOWAIT",
        CicsPlanOption::Rrn => "RRN",
        CicsPlanOption::Console => "CONSOLE",
        CicsPlanOption::PrintMedium => "PRINT",
        CicsPlanOption::Card => "CARD",
        CicsPlanOption::WpMedia1 => "WPMEDIA1",
        CicsPlanOption::WpMedia2 => "WPMEDIA2",
        CicsPlanOption::WpMedia3 => "WPMEDIA3",
        CicsPlanOption::Nleom => "NLEOM",
        CicsPlanOption::WpMedia4 => "WPMEDIA4",
        CicsPlanOption::DigestHex => "DIGESTHEX",
        CicsPlanOption::DigestBinary => "DIGESTBINARY",
        CicsPlanOption::DigestBase64 => "DIGESTBASE64",
        CicsPlanOption::OperatorImmediate => "IMMEDIATE",
        CicsPlanOption::OperatorEventual => "EVENTUAL",
        CicsPlanOption::OperatorCritical => "CRITICAL",
        CicsPlanOption::CertificateOwner => "OWNER",
        CicsPlanOption::CertificateIssuer => "ISSUER",
        CicsPlanOption::Cancel => "CANCEL",
        CicsPlanOption::SecurityBasicAuth => "BASICAUTH",
        CicsPlanOption::SecurityJwt => "JWT",
        CicsPlanOption::SecurityKerberos => "KERBEROS",
        CicsPlanOption::SecurityBit => "BIT",
        CicsPlanOption::SecurityBase64 => "BASE64",
        CicsPlanOption::NoDump => "NODUMP",
        CicsPlanOption::Reset => "RESET",
        CicsPlanOption::Update => "UPDATE",
        CicsPlanOption::Rollback => "ROLLBACK",
        CicsPlanOption::NoHandle => "NOHANDLE",
        CicsPlanOption::AsIs => "ASIS",
        CicsPlanOption::Accum => "ACCUM",
        CicsPlanOption::Formfeed => "FORMFEED",
        CicsPlanOption::DefaultScreen => "DEFAULT",
        CicsPlanOption::AlternateScreen => "ALTERNATE",
        CicsPlanOption::EraseAup => "ERASEAUP",
        CicsPlanOption::Print => "PRINT",
        CicsPlanOption::Alarm => "ALARM",
        CicsPlanOption::Frset => "FRSET",
        CicsPlanOption::Paging => "PAGING",
        CicsPlanOption::Last => "LAST",
        CicsPlanOption::Honeom => "HONEOM",
        CicsPlanOption::L40 => "L40",
        CicsPlanOption::L64 => "L64",
        CicsPlanOption::L80 => "L80",
        CicsPlanOption::ReleasePage => "RELEASE",
        CicsPlanOption::RetainPage => "RETAIN",
        CicsPlanOption::Autopage => "AUTOPAGE",
        CicsPlanOption::CurrentPage => "CURRENT",
        CicsPlanOption::AllPages => "ALL",
        CicsPlanOption::NoAutopage => "NOAUTOPAGE",
        CicsPlanOption::OperPurge => "OPERPURGE",
        CicsPlanOption::Task => "TASK",
        CicsPlanOption::Uow => "UOW",
        CicsPlanOption::NoSuspend => "NOSUSPEND",
        CicsPlanOption::CounterNoSuspend => "NOSUSPEND",
        CicsPlanOption::CounterReduce => "REDUCE",
        CicsPlanOption::CounterWrap => "WRAP",
        CicsPlanOption::Erase => "ERASE",
        CicsPlanOption::Cursor => "CURSOR",
        CicsPlanOption::DateSep => "DATESEP",
        CicsPlanOption::TimeSep => "TIMESEP",
        CicsPlanOption::FreeKb => "FREEKB",
        CicsPlanOption::Gteq => "GTEQ",
        CicsPlanOption::Fmh => "FMH",
        CicsPlanOption::Protect => "PROTECT",
        CicsPlanOption::Wait => "WAIT",
        CicsPlanOption::After => "AFTER",
        CicsPlanOption::At => "AT",
        CicsPlanOption::For => "FOR",
        CicsPlanOption::Until => "UNTIL",
        CicsPlanOption::NoCheck => "NOCHECK",
        CicsPlanOption::MapOnly => "MAPONLY",
        CicsPlanOption::DataOnly => "DATAONLY",
        CicsPlanOption::DocumentDataOnly => "DATAONLY",
        CicsPlanOption::Generic => "GENERIC",
        CicsPlanOption::Equal => "EQUAL",
        CicsPlanOption::Terminal => "TERMINAL",
        CicsPlanOption::Purgeable => "PURGEABLE",
        CicsPlanOption::NotPurgeable => "NOTPURGEABLE",
        CicsPlanOption::Next => "NEXT",
        CicsPlanOption::RewriteTemporary => "REWRITE",
        CicsPlanOption::Auxiliary => "AUXILIARY",
        CicsPlanOption::Main => "MAIN",
        CicsPlanOption::ExactMatch => "EXACTMATCH",
        CicsPlanOption::Minimum => "MINIMUM",
        CicsPlanOption::Hold => "HOLD",
        CicsPlanOption::Unescaped => "UNESCAPED",
        CicsPlanOption::CicsDataKey64 => "CICSDATAKEY",
        CicsPlanOption::UserDataKey64 => "USERDATAKEY",
        CicsPlanOption::Shared64 => "SHARED",
        CicsPlanOption::Executable64 => "EXECUTABLE",
        CicsPlanOption::SpoolKeep => "KEEP",
        CicsPlanOption::SpoolDelete => "DELETE",
        CicsPlanOption::SpoolNoCc => "NOCC",
        CicsPlanOption::SpoolAsa => "ASA",
        CicsPlanOption::SpoolMcc => "MCC",
        CicsPlanOption::SpoolPrint => "PRINT",
        CicsPlanOption::SpoolPunch => "PUNCH",
        CicsPlanOption::SpoolLine => "LINE",
        CicsPlanOption::SpoolPage => "PAGE",
        CicsPlanOption::EventAnd => "AND",
        CicsPlanOption::EventOr => "OR",
        CicsPlanOption::TimerAfter => "AFTER",
        CicsPlanOption::TimerAt => "AT",
        CicsPlanOption::TimerOn => "ON",
        CicsPlanOption::AcqActivity => "ACQACTIVITY",
        CicsPlanOption::AcqProcess => "ACQPROCESS",
        CicsPlanOption::TraceException => "EXCEPTION",
        CicsPlanOption::DumpComplete => "COMPLETE",
        CicsPlanOption::DumpTask => "TASK",
        CicsPlanOption::DumpStorage => "STORAGE",
        CicsPlanOption::DumpProgram => "PROGRAM",
        CicsPlanOption::DumpTerminal => "TERMINAL",
        CicsPlanOption::DumpTables => "TABLES",
        CicsPlanOption::DumpFct => "FCT",
        CicsPlanOption::DumpPct => "PCT",
        CicsPlanOption::DumpPpt => "PPT",
        CicsPlanOption::DumpSit => "SIT",
        CicsPlanOption::DumpTct => "TCT",
        CicsPlanOption::DumpTrt => "TRT",
        CicsPlanOption::DumpDct => "DCT",
        CicsPlanOption::TraceOn => "ON",
        CicsPlanOption::TraceOff => "OFF",
        CicsPlanOption::TraceSystem => "SYSTEM",
        CicsPlanOption::TraceUser => "USER",
        CicsPlanOption::TraceEi => "EI",
        CicsPlanOption::TraceSingle => "SINGLE",
        CicsPlanOption::TraceAccount => "ACCOUNT",
        CicsPlanOption::TraceMonitor => "MONITOR",
        CicsPlanOption::TracePerform => "PERFORM",
        CicsPlanOption::WebBrowseHttpHeader => "HTTPHEADER",
        CicsPlanOption::WebBrowseQueryParm => "QUERYPARM",
        CicsPlanOption::WebBrowseFormField => "FORMFIELD",
        CicsPlanOption::WebNotruncate => "NOTRUNCATE",
        CicsPlanOption::IssueWaitOption => "WAIT",
        CicsPlanOption::IssueEndOutput => "ENDOUTPUT",
        CicsPlanOption::IssueEndFile => "ENDFILE",
        CicsPlanOption::IssueConverse => "CONVERSE",
        CicsPlanOption::IssueLogonLogmode => "LOGONLOGMODE",
        CicsPlanOption::IssueNoQuiesce => "NOQUIESCE",
        CicsPlanOption::WebNoClientConvert => "NOCLICONVERT",
        CicsPlanOption::WebNoServerConvert => "NOSRVCONVERT",
    }
}

#[cfg(test)]
mod conversation_tests {
    use super::*;

    #[test]
    fn conversation_plans_route_mapped_forms_only() {
        for (plan, expected) in [
            (
                CicsPlanOperation::AllocateConversation,
                CicsOperation::AllocateConversation,
            ),
            (CicsPlanOperation::BuildAttach, CicsOperation::BuildAttach),
            (
                CicsPlanOperation::ConnectProcess,
                CicsOperation::ConnectProcess,
            ),
            (CicsPlanOperation::Converse, CicsOperation::Converse),
            (
                CicsPlanOperation::FreeConversation,
                CicsOperation::FreeConversation,
            ),
        ] {
            assert_eq!(host_operation(plan), Some(expected));
        }
        for operation in [
            CicsPlanOperation::GdsAllocateConversation,
            CicsPlanOperation::GdsAssignConversation,
            CicsPlanOperation::GdsConnectProcess,
            CicsPlanOperation::GdsFreeConversation,
        ] {
            assert_eq!(host_operation(operation), None);
        }
        assert_eq!(
            host_operation(CicsPlanOperation::Read),
            Some(CicsOperation::Read)
        );
    }
}
