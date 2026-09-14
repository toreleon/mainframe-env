use mainframe_env_host_api::CicsOperation;
use mainframe_env_ir::{
    CicsAssignOutput, CicsOperandName, CicsOutputName, CicsPlanOperation, CicsPlanOption,
};

#[derive(Clone, Copy)]
pub(super) enum SlotUse {
    Input,
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
        _ => SlotUse::Input,
    }
}

pub(super) const fn output_slot_use(name: CicsOutputName) -> SlotUse {
    match name {
        CicsOutputName::Abstime => SlotUse::AbstimeOutput,
        CicsOutputName::Commarea => SlotUse::Output,
        CicsOutputName::Into => SlotUse::Output,
        CicsOutputName::Ridfld => SlotUse::Output,
        CicsOutputName::Milliseconds => SlotUse::MillisecondsOutput,
        CicsOutputName::Mmddyy | CicsOutputName::Time | CicsOutputName::Yymmdd => {
            SlotUse::FormatTextOutput(8)
        }
        CicsOutputName::Mmddyyyy | CicsOutputName::Yyyymmdd => SlotUse::FormatTextOutput(10),
        CicsOutputName::Yyddd => SlotUse::FormatTextOutput(6),
        CicsOutputName::Resp | CicsOutputName::Resp2 => SlotUse::NumericOutput,
        CicsOutputName::Assign(output) => SlotUse::AssignOutput(output),
    }
}

pub(super) const fn host_operation(operation: CicsPlanOperation) -> CicsOperation {
    match operation {
        CicsPlanOperation::Abend => CicsOperation::Abend,
        CicsPlanOperation::AddressSet => CicsOperation::AddressSet,
        CicsPlanOperation::Asktime => CicsOperation::Asktime,
        CicsPlanOperation::AsktimeEib => CicsOperation::AsktimeEib,
        CicsPlanOperation::FormatTime => CicsOperation::FormatTime,
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
        CicsPlanOperation::EndBrowse => CicsOperation::EndBrowse,
        CicsPlanOperation::Delete => CicsOperation::Delete,
        CicsPlanOperation::Write => CicsOperation::Write,
        CicsPlanOperation::WriteTransientData => CicsOperation::WriteTransientData,
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
    }
}

pub(super) const fn operand(name: CicsOperandName) -> &'static str {
    match name {
        CicsOperandName::Abcode => "ABCODE",
        CicsOperandName::Label => "LABEL",
        CicsOperandName::Program => "PROGRAM",
        CicsOperandName::Commarea => "COMMAREA",
        CicsOperandName::TransId => "TRANSID",
        CicsOperandName::File => "FILE",
        CicsOperandName::Dataset => "DATASET",
        CicsOperandName::From => "FROM",
        CicsOperandName::Ridfld => "RIDFLD",
        CicsOperandName::Queue => "QUEUE",
        CicsOperandName::Map => "MAP",
        CicsOperandName::Mapset => "MAPSET",
        CicsOperandName::Resource => "RESOURCE",
        CicsOperandName::Length => "LENGTH",
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
    }
}

pub(super) const fn output(name: CicsOutputName) -> &'static str {
    match name {
        CicsOutputName::Abstime => "ABSTIME",
        CicsOutputName::Commarea => "COMMAREA",
        CicsOutputName::Into => "INTO",
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
    }
}
