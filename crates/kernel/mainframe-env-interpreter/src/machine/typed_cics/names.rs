use mainframe_env_host_api::CicsOperation;
use mainframe_env_ir::{CicsOperandName, CicsOutputName, CicsPlanOperation, CicsPlanOption};

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
        CicsPlanOperation::HandleCondition => CicsOperation::HandleCondition,
        CicsPlanOperation::IgnoreCondition => CicsOperation::IgnoreCondition,
        CicsPlanOperation::PopHandle => CicsOperation::PopHandle,
        CicsPlanOperation::PushHandle => CicsOperation::PushHandle,
        CicsPlanOperation::Read => CicsOperation::Read,
        CicsPlanOperation::Rewrite => CicsOperation::Rewrite,
        CicsPlanOperation::SetAssociationUserCorrData => CicsOperation::SetAssociationUserCorrData,
        CicsPlanOperation::Syncpoint => CicsOperation::Syncpoint,
        CicsPlanOperation::Suspend => CicsOperation::Suspend,
    }
}

pub(super) const fn operand(name: CicsOperandName) -> &'static str {
    match name {
        CicsOperandName::Abcode => "ABCODE",
        CicsOperandName::File => "FILE",
        CicsOperandName::Dataset => "DATASET",
        CicsOperandName::From => "FROM",
        CicsOperandName::Ridfld => "RIDFLD",
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
        CicsOutputName::Into => "INTO",
        CicsOutputName::Milliseconds => "MILLISECONDS",
        CicsOutputName::Mmddyy => "MMDDYY",
        CicsOutputName::Mmddyyyy => "MMDDYYYY",
        CicsOutputName::Resp => "RESP",
        CicsOutputName::Resp2 => "RESP2",
        CicsOutputName::Time => "TIME",
        CicsOutputName::Yyddd => "YYDDD",
        CicsOutputName::Yymmdd => "YYMMDD",
        CicsOutputName::Yyyymmdd => "YYYYMMDD",
    }
}

pub(super) const fn option(option: CicsPlanOption) -> &'static str {
    match option {
        CicsPlanOption::Cancel => "CANCEL",
        CicsPlanOption::NoDump => "NODUMP",
        CicsPlanOption::Update => "UPDATE",
        CicsPlanOption::Rollback => "ROLLBACK",
        CicsPlanOption::NoHandle => "NOHANDLE",
        CicsPlanOption::Task => "TASK",
        CicsPlanOption::Uow => "UOW",
        CicsPlanOption::NoSuspend => "NOSUSPEND",
    }
}
