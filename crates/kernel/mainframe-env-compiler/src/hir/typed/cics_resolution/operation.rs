use super::super::{HirCicsOperation, Resolution, ResolutionFailure};
use mainframe_env_ir::CicsApplicationRegistryDescriptor;

pub(super) fn resolve(
    descriptor: &CicsApplicationRegistryDescriptor,
) -> Resolution<HirCicsOperation> {
    Ok(match descriptor.label_tokens {
        ["ABEND"] => HirCicsOperation::Abend,
        ["ADDRESS", "SET"] => HirCicsOperation::AddressSet,
        ["ASKTIME", "ABSTIME"] => HirCicsOperation::Asktime,
        ["ASKTIME"] => HirCicsOperation::AsktimeEib,
        ["FORMATTIME"] => HirCicsOperation::FormatTime,
        ["CHANGE", "TASK"] => HirCicsOperation::ChangeTask,
        ["DEQ"] => HirCicsOperation::Deq,
        ["ENQ"] => HirCicsOperation::Enq,
        ["HANDLE", "ABEND"] => HirCicsOperation::HandleAbend,
        ["HANDLE", "AID"] => HirCicsOperation::HandleAid,
        ["HANDLE", "CONDITION"] => HirCicsOperation::HandleCondition,
        ["IGNORE", "CONDITION"] => HirCicsOperation::IgnoreCondition,
        ["LINK"] => HirCicsOperation::Link,
        ["XCTL"] => HirCicsOperation::Xctl,
        ["RETURN"] => HirCicsOperation::Return,
        ["STARTBR"] => HirCicsOperation::StartBrowse,
        ["READNEXT"] => HirCicsOperation::ReadNext,
        ["READPREV"] => HirCicsOperation::ReadPrev,
        ["ENDBR"] => HirCicsOperation::EndBrowse,
        ["DELETE"] => HirCicsOperation::Delete,
        ["WRITE", "FILE"] => HirCicsOperation::Write,
        ["WRITEQ", "TD"] => HirCicsOperation::WriteTransientData,
        ["RECEIVE", "MAP"] => HirCicsOperation::ReceiveMap,
        ["SEND", "MAP"] => HirCicsOperation::SendMap,
        ["SEND", "TEXT"] => HirCicsOperation::SendText,
        ["POP", "HANDLE"] => HirCicsOperation::PopHandle,
        ["PUSH", "HANDLE"] => HirCicsOperation::PushHandle,
        ["READ"] => HirCicsOperation::Read,
        ["REWRITE"] => HirCicsOperation::Rewrite,
        ["SET", "ASSOCIATION", "USERCORRDATA"] => HirCicsOperation::SetAssociationUserCorrData,
        ["SYNCPOINT"] => HirCicsOperation::Syncpoint,
        ["SUSPEND"] => HirCicsOperation::Suspend,
        _ => return Err(ResolutionFailure::Unsupported),
    })
}
