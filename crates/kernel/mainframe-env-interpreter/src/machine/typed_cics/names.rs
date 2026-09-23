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
            | CicsPlanOperation::QueryCounter,
        ) => Some((4, true)),
        Some(
            CicsPlanOperation::DefineDCounter
            | CicsPlanOperation::DeleteDCounter
            | CicsPlanOperation::GetDCounter
            | CicsPlanOperation::QueryDCounter,
        ) => Some((8, false)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(super) enum SlotUse {
    Input,
    HalfwordInput,
    FullwordInput,
    CounterNumber,
    AbcodeInput,
    ProgramNameInput,
    AbstimeInput,
    SeparatorInput,
    Output,
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
}

pub(super) const fn input_slot_use(name: CicsOperandName) -> SlotUse {
    match name {
        CicsOperandName::Abcode => SlotUse::AbcodeInput,
        CicsOperandName::Program => SlotUse::ProgramNameInput,
        CicsOperandName::Abstime => SlotUse::AbstimeInput,
        CicsOperandName::MajorVersion | CicsOperandName::MinorVersion => SlotUse::FullwordInput,
        CicsOperandName::DateSep | CicsOperandName::TimeSep => SlotUse::SeparatorInput,
        CicsOperandName::KeyLength => SlotUse::Input,
        CicsOperandName::Flength
        | CicsOperandName::ElementNameLength
        | CicsOperandName::ElementNamespaceLength
        | CicsOperandName::TypeNameLength
        | CicsOperandName::TypeNamespaceLength
        | CicsOperandName::JournalReqId => SlotUse::FullwordInput,
        CicsOperandName::JournalFlength => SlotUse::FullwordInput,
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
        _ => SlotUse::Input,
    }
}

pub(super) const fn output_slot_use(name: CicsOutputName) -> SlotUse {
    match name {
        CicsOutputName::CounterValue
        | CicsOutputName::CounterMinimum
        | CicsOutputName::CounterMaximum => SlotUse::CounterNumber,
        CicsOutputName::Abstime => SlotUse::AbstimeOutput,
        CicsOutputName::Commarea => SlotUse::Output,
        CicsOutputName::Into => SlotUse::Output,
        CicsOutputName::SetPointer => SlotUse::PointerOutput,
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
    }
}

pub(super) const fn host_operation(operation: CicsPlanOperation) -> CicsOperation {
    match operation {
        CicsPlanOperation::Abend => CicsOperation::Abend,
        CicsPlanOperation::Address => CicsOperation::Address,
        CicsPlanOperation::AddressSet => CicsOperation::AddressSet,
        CicsPlanOperation::Asktime => CicsOperation::Asktime,
        CicsPlanOperation::AsktimeEib => CicsOperation::AsktimeEib,
        CicsPlanOperation::FormatTime => CicsOperation::FormatTime,
        CicsPlanOperation::Cancel => CicsOperation::Cancel,
        CicsPlanOperation::Delay => CicsOperation::Delay,
        CicsPlanOperation::DefineCounter => CicsOperation::DefineCounter,
        CicsPlanOperation::DefineDCounter => CicsOperation::DefineDCounter,
        CicsPlanOperation::DeleteCounter => CicsOperation::DeleteCounter,
        CicsPlanOperation::DeleteDCounter => CicsOperation::DeleteDCounter,
        CicsPlanOperation::GetCounter => CicsOperation::GetCounter,
        CicsPlanOperation::GetDCounter => CicsOperation::GetDCounter,
        CicsPlanOperation::QueryCounter => CicsOperation::QueryCounter,
        CicsPlanOperation::QueryDCounter => CicsOperation::QueryDCounter,
        CicsPlanOperation::ChangeTask => CicsOperation::ChangeTask,
        CicsPlanOperation::Deq => CicsOperation::Deq,
        CicsPlanOperation::Enq => CicsOperation::Enq,
        CicsPlanOperation::HandleAid => CicsOperation::HandleAid,
        CicsPlanOperation::HandleAbend => CicsOperation::HandleAbend,
        CicsPlanOperation::HandleCondition => CicsOperation::HandleCondition,
        CicsPlanOperation::IgnoreCondition => CicsOperation::IgnoreCondition,
        CicsPlanOperation::InvokeApplication => CicsOperation::InvokeApplication,
        CicsPlanOperation::Load => CicsOperation::Load,
        CicsPlanOperation::Release => CicsOperation::Release,
        CicsPlanOperation::Link => CicsOperation::Link,
        CicsPlanOperation::Xctl => CicsOperation::Xctl,
        CicsPlanOperation::Return => CicsOperation::Return,
        CicsPlanOperation::StartBrowse => CicsOperation::StartBrowse,
        CicsPlanOperation::ResetBrowse => CicsOperation::ResetBrowse,
        CicsPlanOperation::Unlock => CicsOperation::Unlock,
        CicsPlanOperation::ReadNext => CicsOperation::ReadNext,
        CicsPlanOperation::ReadPrev => CicsOperation::ReadPrev,
        CicsPlanOperation::ReadTransientData => CicsOperation::ReadTransientData,
        CicsPlanOperation::EndBrowse => CicsOperation::EndBrowse,
        CicsPlanOperation::Delete => CicsOperation::Delete,
        CicsPlanOperation::Write => CicsOperation::Write,
        CicsPlanOperation::WriteTransientData => CicsOperation::WriteTransientData,
        CicsPlanOperation::DeleteTransientData => CicsOperation::DeleteTransientData,
        CicsPlanOperation::DeleteTemporaryStorage => CicsOperation::DeleteTemporaryStorage,
        CicsPlanOperation::ReadTemporaryStorage => CicsOperation::ReadTemporaryStorage,
        CicsPlanOperation::WriteTemporaryStorage => CicsOperation::WriteTemporaryStorage,
        CicsPlanOperation::Getmain => CicsOperation::Getmain,
        CicsPlanOperation::Getmain64 => CicsOperation::Getmain64,
        CicsPlanOperation::Freemain => CicsOperation::Freemain,
        CicsPlanOperation::Freemain64 => CicsOperation::Freemain64,
        CicsPlanOperation::ReceiveMap => CicsOperation::ReceiveMap,
        CicsPlanOperation::SendMap => CicsOperation::SendMap,
        CicsPlanOperation::SendText => CicsOperation::SendText,
        CicsPlanOperation::PopHandle => CicsOperation::PopHandle,
        CicsPlanOperation::PushHandle => CicsOperation::PushHandle,
        CicsPlanOperation::Read => CicsOperation::Read,
        CicsPlanOperation::Rewrite => CicsOperation::Rewrite,
        CicsPlanOperation::SetAssociationUserCorrData => CicsOperation::SetAssociationUserCorrData,
        CicsPlanOperation::SpoolClose => CicsOperation::SpoolClose,
        CicsPlanOperation::SpoolOpenInput => CicsOperation::SpoolOpenInput,
        CicsPlanOperation::SpoolOpenOutput => CicsOperation::SpoolOpenOutput,
        CicsPlanOperation::SpoolRead => CicsOperation::SpoolRead,
        CicsPlanOperation::SpoolWrite => CicsOperation::SpoolWrite,
        CicsPlanOperation::Syncpoint => CicsOperation::Syncpoint,
        CicsPlanOperation::Suspend => CicsOperation::Suspend,
        CicsPlanOperation::WaitEvent => CicsOperation::WaitEvent,
        CicsPlanOperation::WaitExternal => CicsOperation::WaitExternal,
        CicsPlanOperation::Assign => CicsOperation::Assign,
        CicsPlanOperation::PurgeMessage => CicsOperation::PurgeMessage,
        CicsPlanOperation::Start => CicsOperation::Start,
        CicsPlanOperation::Retrieve => CicsOperation::Retrieve,
        CicsPlanOperation::DocumentCreate => CicsOperation::DocumentCreate,
        CicsPlanOperation::DocumentDelete => CicsOperation::DocumentDelete,
        CicsPlanOperation::DocumentInsert => CicsOperation::DocumentInsert,
        CicsPlanOperation::DocumentRetrieve => CicsOperation::DocumentRetrieve,
        CicsPlanOperation::DocumentSet => CicsOperation::DocumentSet,
        CicsPlanOperation::InvokeService => CicsOperation::InvokeService,
        CicsPlanOperation::SoapFaultAdd => CicsOperation::SoapFaultAdd,
        CicsPlanOperation::SoapFaultCreate => CicsOperation::SoapFaultCreate,
        CicsPlanOperation::SoapFaultDelete => CicsOperation::SoapFaultDelete,
        CicsPlanOperation::WsaContextBuild => CicsOperation::WsaContextBuild,
        CicsPlanOperation::WsaContextDelete => CicsOperation::WsaContextDelete,
        CicsPlanOperation::WsaContextGet => CicsOperation::WsaContextGet,
        CicsPlanOperation::WsaEprCreate => CicsOperation::WsaEprCreate,
        CicsPlanOperation::TransformDataToJson => CicsOperation::TransformDataToJson,
        CicsPlanOperation::TransformDataToXml => CicsOperation::TransformDataToXml,
        CicsPlanOperation::TransformJsonToData => CicsOperation::TransformJsonToData,
        CicsPlanOperation::TransformXmlToData => CicsOperation::TransformXmlToData,
        CicsPlanOperation::WaitJournalName => CicsOperation::WaitJournalName,
        CicsPlanOperation::WaitJournalNum => CicsOperation::WaitJournalNum,
        CicsPlanOperation::WriteJournalName => CicsOperation::WriteJournalName,
        CicsPlanOperation::WriteJournalNum => CicsOperation::WriteJournalNum,
    }
}

pub(super) const fn operand(name: CicsOperandName) -> &'static str {
    match name {
        CicsOperandName::Abcode => "ABCODE",
        CicsOperandName::Label => "LABEL",
        CicsOperandName::Program => "PROGRAM",
        CicsOperandName::Commarea => "COMMAREA",
        CicsOperandName::TransId => "TRANSID",
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
    ) && matches!(name, CicsOperandName::CounterName)
    {
        "DCOUNTER"
    } else {
        operand(name)
    }
}

pub(super) const fn output(name: CicsOutputName) -> &'static str {
    match name {
        CicsOutputName::CounterValue => "VALUE",
        CicsOutputName::CounterMinimum => "MINIMUM",
        CicsOutputName::CounterMaximum => "MAXIMUM",
        CicsOutputName::Abstime => "ABSTIME",
        CicsOutputName::Commarea => "COMMAREA",
        CicsOutputName::Into => "INTO",
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
    }
}

pub(super) const fn option(option: CicsPlanOption) -> &'static str {
    match option {
        CicsPlanOption::Cancel => "CANCEL",
        CicsPlanOption::NoDump => "NODUMP",
        CicsPlanOption::Reset => "RESET",
        CicsPlanOption::Update => "UPDATE",
        CicsPlanOption::Rollback => "ROLLBACK",
        CicsPlanOption::NoHandle => "NOHANDLE",
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
    }
}
