use super::{CicsOutputName, CicsPlanOperation};

pub(super) const fn allowed(operation: CicsPlanOperation, output: CicsOutputName) -> bool {
    match operation {
        CicsPlanOperation::Asktime => matches!(
            output,
            CicsOutputName::Abstime | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ConvertTime => matches!(
            output,
            CicsOutputName::Abstime | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::BifDeedit => matches!(
            output,
            CicsOutputName::Field | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::BifDigest => matches!(
            output,
            CicsOutputName::DigestResult | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Post => matches!(
            output,
            CicsOutputName::SetPointer | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WriteOperator => matches!(
            output,
            CicsOutputName::OperatorReply
                | CicsOutputName::OperatorReplyLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ExtractCertificate => matches!(
            output,
            CicsOutputName::Certificate(_) | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::ExtractTcpip => matches!(
            output,
            CicsOutputName::Tcpip(_) | CicsOutputName::Resp | CicsOutputName::Resp2
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
        CicsPlanOperation::ReceivePartn => matches!(
            output,
            CicsOutputName::Partn
                | CicsOutputName::Into
                | CicsOutputName::SetPointer
                | CicsOutputName::Length
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::SendControl | CicsPlanOperation::SendPage => matches!(
            output,
            CicsOutputName::SetPointer | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::IssueNote => matches!(
            output,
            CicsOutputName::Ridfld | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::IssueReceive => matches!(
            output,
            CicsOutputName::Into
                | CicsOutputName::SetPointer
                | CicsOutputName::Length
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
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
        CicsPlanOperation::WebParseUrl => matches!(
            output,
            CicsOutputName::WebSchemeName
                | CicsOutputName::WebHost
                | CicsOutputName::WebHostLength
                | CicsOutputName::WebHostType
                | CicsOutputName::WebPortNumber
                | CicsOutputName::WebPath
                | CicsOutputName::WebPathLength
                | CicsOutputName::WebQueryString
                | CicsOutputName::WebQueryStringLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebOpen => matches!(
            output,
            CicsOutputName::WebSessionToken
                | CicsOutputName::WebHttpVNum
                | CicsOutputName::WebHttpRNum
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebExtract | CicsPlanOperation::ExtractWeb => matches!(
            output,
            CicsOutputName::WebScheme
                | CicsOutputName::WebHost
                | CicsOutputName::WebHostLength
                | CicsOutputName::WebHostType
                | CicsOutputName::WebHttpMethod
                | CicsOutputName::WebMethodLength
                | CicsOutputName::WebHttpVersion
                | CicsOutputName::WebVersionLength
                | CicsOutputName::WebPath
                | CicsOutputName::WebPathLength
                | CicsOutputName::WebPortNumber
                | CicsOutputName::WebQueryString
                | CicsOutputName::WebQueryStringLength
                | CicsOutputName::WebRequestType
                | CicsOutputName::WebUriMap
                | CicsOutputName::WebRealm
                | CicsOutputName::WebRealmLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebRead => matches!(
            output,
            CicsOutputName::WebValue
                | CicsOutputName::WebValueLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebReadNext => matches!(
            output,
            CicsOutputName::WebBrowseName
                | CicsOutputName::WebBrowseNameLength
                | CicsOutputName::WebValue
                | CicsOutputName::WebValueLength
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebRetrieve => matches!(
            output,
            CicsOutputName::WebRetrieveDocumentToken | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebReceive => matches!(
            output,
            CicsOutputName::WebReceiveInto
                | CicsOutputName::WebReceiveLength
                | CicsOutputName::WebReceiveStatusCode
                | CicsOutputName::WebReceiveStatusText
                | CicsOutputName::WebReceiveStatusLength
                | CicsOutputName::WebReceiveMediaType
                | CicsOutputName::WebReceiveBodyCharset
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::WebConverse => matches!(
            output,
            CicsOutputName::WebConverseInto
                | CicsOutputName::WebConverseToLength
                | CicsOutputName::WebConverseStatusCode
                | CicsOutputName::WebConverseStatusText
                | CicsOutputName::WebConverseStatusLength
                | CicsOutputName::WebConverseMediaType
                | CicsOutputName::WebConverseBodyCharset
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
        CicsPlanOperation::CheckTimer => matches!(
            output,
            CicsOutputName::TimerStatus | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::RetrieveReattachEvent => matches!(
            output,
            CicsOutputName::EventName
                | CicsOutputName::EventType
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::RetrieveSubevent => matches!(
            output,
            CicsOutputName::SubEventName
                | CicsOutputName::EventType
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::TestEvent => matches!(
            output,
            CicsOutputName::FireStatus | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::DumpTransaction => matches!(
            output,
            CicsOutputName::DumpId | CicsOutputName::Resp | CicsOutputName::Resp2
        ),
        CicsPlanOperation::VerifyToken => matches!(
            output,
            CicsOutputName::SecurityIsUserId
                | CicsOutputName::SecurityEncryptKey
                | CicsOutputName::SecurityOutToken
                | CicsOutputName::SecurityOutTokenLength
                | CicsOutputName::SecurityEsmResp
                | CicsOutputName::SecurityEsmReason
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::RequestEncryptPassTicket => matches!(
            output,
            CicsOutputName::SecurityEncryptPassTicket
                | CicsOutputName::SecurityEncryptLength
                | CicsOutputName::SecurityEsmResp
                | CicsOutputName::SecurityEsmReason
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::QuerySecurity => matches!(
            output,
            CicsOutputName::SecurityRead
                | CicsOutputName::SecurityUpdate
                | CicsOutputName::SecurityControl
                | CicsOutputName::SecurityAlter
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::VerifyPassword
        | CicsPlanOperation::VerifyPhrase
        | CicsPlanOperation::ChangePassword
        | CicsPlanOperation::ChangePhrase => matches!(
            output,
            CicsOutputName::SecurityChangeTime
                | CicsOutputName::SecurityDaysLeft
                | CicsOutputName::SecurityEsmReason
                | CicsOutputName::SecurityEsmResp
                | CicsOutputName::SecurityExpiryTime
                | CicsOutputName::SecurityInvalidCount
                | CicsOutputName::SecurityLastUseTime
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::RequestPassTicket => matches!(
            output,
            CicsOutputName::SecurityPassTicket
                | CicsOutputName::SecurityEsmResp
                | CicsOutputName::SecurityEsmReason
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        CicsPlanOperation::Signon => matches!(
            output,
            CicsOutputName::SecurityChangeTime
                | CicsOutputName::SecurityDaysLeft
                | CicsOutputName::SecurityEsmReason
                | CicsOutputName::SecurityEsmResp
                | CicsOutputName::SecurityExpiryTime
                | CicsOutputName::SecurityInvalidCount
                | CicsOutputName::SecurityLastUseTime
                | CicsOutputName::SecurityLangInUse
                | CicsOutputName::SecurityNatLangInUse
                | CicsOutputName::Resp
                | CicsOutputName::Resp2
        ),
        _ => matches!(output, CicsOutputName::Resp | CicsOutputName::Resp2),
    }
}
