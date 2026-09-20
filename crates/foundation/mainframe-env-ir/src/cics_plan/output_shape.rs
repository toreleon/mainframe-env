use super::{CicsOutputName, CicsPlanOperation};

pub(super) const fn allowed(operation: CicsPlanOperation, output: CicsOutputName) -> bool {
    match operation {
        CicsPlanOperation::Asktime => matches!(
            output,
            CicsOutputName::Abstime | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Read | CicsPlanOperation::Retrieve => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::SetPointer
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
                | CicsOutputName::Length
                | CicsOutputName::ReturnTransId
                | CicsOutputName::ReturnTermId
                | CicsOutputName::Queue
        ),
        CicsPlanOperation::Getmain => matches!(
            output,
            CicsOutputName::SetPointer | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::FormatTime => matches!(
            output,
            CicsOutputName::Milliseconds
                | CicsOutputName::Mmddyy
                | CicsOutputName::Mmddyyyy
                | CicsOutputName::Time
                | CicsOutputName::Yyddd
                | CicsOutputName::Yymmdd
                | CicsOutputName::Yyyymmdd
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Link => matches!(
            output,
            CicsOutputName::Commarea | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ReadNext | CicsPlanOperation::ReadPrev => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::Ridfld
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ReceiveMap => matches!(
            output,
            CicsOutputName::Into | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Assign => matches!(
            output,
            CicsOutputName::Assign(_) | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        _ => matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2),
    }
}
