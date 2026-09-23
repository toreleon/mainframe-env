use super::{CicsOutputName, CicsPlanOperation};

pub(super) const fn allowed(operation: CicsPlanOperation, output: CicsOutputName) -> bool {
    match operation {
        CicsPlanOperation::Asktime => matches!(
            output,
            CicsOutputName::Abstime | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Read | CicsPlanOperation::Retrieve => {
            matches!(
                output,
                CicsOutputName::Into
                    | CicsOutputName::SetPointer
                    | CicsOutputName::Resp
                    | CicsOutputName::Resp2
                    | CicsOutputName::Length
                    | CicsOutputName::ReturnTransId
                    | CicsOutputName::ReturnTermId
                    | CicsOutputName::Queue
            ) || matches!(
                (operation, output),
                (CicsPlanOperation::Read, CicsOutputName::Token)
            )
        }
        CicsPlanOperation::ReadTemporaryStorage => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::SetPointer
                | CicsOutputName::Length
                | CicsOutputName::NumItems
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WriteTemporaryStorage => matches!(
            output,
            CicsOutputName::NumItems | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Getmain => matches!(
            output,
            CicsOutputName::SetPointer | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Getmain64 => matches!(
            output,
            CicsOutputName::SetPointer64 | CicsOutputName::Resp | CicsOutputName::Resp2
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
        CicsPlanOperation::Link | CicsPlanOperation::InvokeApplication => matches!(
            output,
            CicsOutputName::Commarea | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ReadNext | CicsPlanOperation::ReadPrev => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::Length
                | CicsOutputName::Ridfld
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ReadTransientData => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::SetPointer
                | CicsOutputName::Length
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
        CicsPlanOperation::DocumentCreate => matches!(
            output,
            CicsOutputName::DocumentToken
                | CicsOutputName::DocumentSize
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::DocumentInsert => matches!(
            output,
            CicsOutputName::DocumentSize | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::DocumentRetrieve => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::Length
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WsaContextGet => matches!(
            output,
            CicsOutputName::WebAction
                | CicsOutputName::WebMessageId
                | CicsOutputName::WebRelatesUri
                | CicsOutputName::WebRelatesType
                | CicsOutputName::WebEprInto
                | CicsOutputName::WebEprSet
                | CicsOutputName::WebEprLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WsaEprCreate => matches!(
            output,
            CicsOutputName::WebEprInto
                | CicsOutputName::WebEprSet
                | CicsOutputName::WebEprLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::TransformDataToXml | CicsPlanOperation::TransformXmlToData => matches!(
            output,
            CicsOutputName::ElementName
                | CicsOutputName::ElementNameLength
                | CicsOutputName::ElementNamespace
                | CicsOutputName::ElementNamespaceLength
                | CicsOutputName::TypeName
                | CicsOutputName::TypeNameLength
                | CicsOutputName::TypeNamespace
                | CicsOutputName::TypeNamespaceLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WaitJournalName | CicsPlanOperation::WaitJournalNum => {
            matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2)
        }
        CicsPlanOperation::WriteJournalName | CicsPlanOperation::WriteJournalNum => matches!(
            output,
            CicsOutputName::JournalReqId | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::SpoolOpenInput => matches!(
            output,
            CicsOutputName::SpoolToken | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::SpoolOpenOutput => matches!(
            output,
            CicsOutputName::SpoolToken | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::SpoolRead => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::SpoolToFlength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::SpoolWrite => {
            matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2)
        }
        CicsPlanOperation::GetCounter | CicsPlanOperation::GetDCounter => matches!(
            output,
            CicsOutputName::CounterValue | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::QueryCounter | CicsPlanOperation::QueryDCounter => matches!(
            output,
            CicsOutputName::CounterValue
                | CicsOutputName::CounterMinimum
                | CicsOutputName::CounterMaximum
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        _ => matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2),
    }
}
