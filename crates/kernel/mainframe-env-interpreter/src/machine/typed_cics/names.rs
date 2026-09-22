use mainframe_env_host_api::CicsOperation;
use mainframe_env_ir::{
    CicsAssignOutput, CicsOperandName, CicsOutputName, CicsPlanOperation, CicsPlanOption,
};

#[derive(Clone, Copy)]
pub(super) enum SlotUse {
    Input,
    HalfwordInput,
    FullwordInput,
    AbcodeInput,
    ProgramNameInput,
    AbstimeInput,
    SeparatorInput,
    Output,
    AbstimeOutput,
    FormatTextOutput(usize),
    MillisecondsOutput,
    NumericOutput,
    PointerInput,
    PointerOutput,
    AddressInput,
    AddressOutput,
    AssignOutput(CicsAssignOutput),
}

pub(super) const fn input_slot_use(name: CicsOperandName) -> SlotUse {
    match name {
        CicsOperandName::Abcode => SlotUse::AbcodeInput,
        CicsOperandName::Program => SlotUse::ProgramNameInput,
        CicsOperandName::Abstime => SlotUse::AbstimeInput,
        CicsOperandName::DateSep | CicsOperandName::TimeSep => SlotUse::SeparatorInput,
        CicsOperandName::KeyLength => SlotUse::Input,
        CicsOperandName::Flength => SlotUse::FullwordInput,
        CicsOperandName::DataPointer => SlotUse::PointerInput,
        CicsOperandName::DataArea => SlotUse::Input,
        _ => SlotUse::Input,
    }
}

pub(super) const fn output_slot_use(name: CicsOutputName) -> SlotUse {
    match name {
        CicsOutputName::Abstime => SlotUse::AbstimeOutput,
        CicsOutputName::Commarea => SlotUse::Output,
        CicsOutputName::Into => SlotUse::Output,
        CicsOutputName::SetPointer => SlotUse::PointerOutput,
        CicsOutputName::Ridfld => SlotUse::Output,
        CicsOutputName::ReturnTransId | CicsOutputName::ReturnTermId | CicsOutputName::Queue => {
            SlotUse::Output
        }
        CicsOutputName::Milliseconds => SlotUse::MillisecondsOutput,
        CicsOutputName::Mmddyy | CicsOutputName::Time | CicsOutputName::Yymmdd => {
            SlotUse::FormatTextOutput(8)
        }
        CicsOutputName::Mmddyyyy | CicsOutputName::Yyyymmdd => SlotUse::FormatTextOutput(10),
        CicsOutputName::Yyddd => SlotUse::FormatTextOutput(6),
        CicsOutputName::Resp | CicsOutputName::Resp2 | CicsOutputName::Length => {
            SlotUse::NumericOutput
        }
        CicsOutputName::Assign(output) => SlotUse::AssignOutput(output),
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
        CicsPlanOperation::ChangeTask => CicsOperation::ChangeTask,
        CicsPlanOperation::Deq => CicsOperation::Deq,
        CicsPlanOperation::Enq => CicsOperation::Enq,
        CicsPlanOperation::HandleAid => CicsOperation::HandleAid,
        CicsPlanOperation::HandleAbend => CicsOperation::HandleAbend,
        CicsPlanOperation::HandleCondition => CicsOperation::HandleCondition,
        CicsPlanOperation::IgnoreCondition => CicsOperation::IgnoreCondition,
        CicsPlanOperation::Link => CicsOperation::Link,
        CicsPlanOperation::Xctl => CicsOperation::Xctl,
        CicsPlanOperation::Return => CicsOperation::Return,
        CicsPlanOperation::StartBrowse => CicsOperation::StartBrowse,
        CicsPlanOperation::ReadNext => CicsOperation::ReadNext,
        CicsPlanOperation::ReadPrev => CicsOperation::ReadPrev,
        CicsPlanOperation::ReadTransientData => CicsOperation::ReadTransientData,
        CicsPlanOperation::EndBrowse => CicsOperation::EndBrowse,
        CicsPlanOperation::Delete => CicsOperation::Delete,
        CicsPlanOperation::Write => CicsOperation::Write,
        CicsPlanOperation::WriteTransientData => CicsOperation::WriteTransientData,
        CicsPlanOperation::DeleteTransientData => CicsOperation::DeleteTransientData,
        CicsPlanOperation::DeleteTemporaryStorage => CicsOperation::DeleteTemporaryStorage,
        CicsPlanOperation::Getmain => CicsOperation::Getmain,
        CicsPlanOperation::Freemain => CicsOperation::Freemain,
        CicsPlanOperation::ReceiveMap => CicsOperation::ReceiveMap,
        CicsPlanOperation::SendMap => CicsOperation::SendMap,
        CicsPlanOperation::SendText => CicsOperation::SendText,
        CicsPlanOperation::PopHandle => CicsOperation::PopHandle,
        CicsPlanOperation::PushHandle => CicsOperation::PushHandle,
        CicsPlanOperation::Read => CicsOperation::Read,
        CicsPlanOperation::Rewrite => CicsOperation::Rewrite,
        CicsPlanOperation::SetAssociationUserCorrData => CicsOperation::SetAssociationUserCorrData,
        CicsPlanOperation::Syncpoint => CicsOperation::Syncpoint,
        CicsPlanOperation::Suspend => CicsOperation::Suspend,
        CicsPlanOperation::Assign => CicsOperation::Assign,
        CicsPlanOperation::PurgeMessage => CicsOperation::PurgeMessage,
        CicsPlanOperation::Start => CicsOperation::Start,
        CicsPlanOperation::Retrieve => CicsOperation::Retrieve,
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
        CicsOperandName::InitImage => "INITIMG",
        CicsOperandName::DataPointer => "DATAPOINTER",
        CicsOperandName::DataArea => "DATA",
    }
}

pub(super) const fn output(name: CicsOutputName) -> &'static str {
    match name {
        CicsOutputName::Abstime => "ABSTIME",
        CicsOutputName::Commarea => "COMMAREA",
        CicsOutputName::Into => "INTO",
        CicsOutputName::SetPointer => "SET",
        CicsOutputName::Ridfld => "RIDFLD",
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
        CicsPlanOption::Generic => "GENERIC",
        CicsPlanOption::Equal => "EQUAL",
        CicsPlanOption::Terminal => "TERMINAL",
    }
}
