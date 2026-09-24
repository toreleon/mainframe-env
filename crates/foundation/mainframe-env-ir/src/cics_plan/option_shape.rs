use super::{CicsEffectPlan, CicsPlanOperation, CicsPlanOption};

pub(super) fn has_unsupported(plan: &CicsEffectPlan) -> bool {
    plan.options.iter().any(|option| match plan.operation {
        CicsPlanOperation::ReceiveConversation
        | CicsPlanOperation::GdsReceiveConversation
        | CicsPlanOperation::SendConversation
        | CicsPlanOperation::GdsWaitConversation
        | CicsPlanOperation::WaitConvid
        | CicsPlanOperation::WaitSignal
        | CicsPlanOperation::WaitTerminal => {
            !super::conversation_data_shape::option_allowed(plan.operation, *option)
        }
        CicsPlanOperation::FetchAny | CicsPlanOperation::FetchChild => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::BtsNoSuspend
        ),
        CicsPlanOperation::FreeChild | CicsPlanOperation::LinkActivity => {
            !matches!(option, CicsPlanOption::NoHandle)
        }
        CicsPlanOperation::LinkAcqActivity => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::BtsAcqActivity
        ),
        CicsPlanOperation::LinkAcqProcess => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::BtsAcqProcess
        ),
        CicsPlanOperation::BifDigest => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::DigestHex
                | CicsPlanOption::DigestBinary
                | CicsPlanOption::DigestBase64
        ),
        CicsPlanOperation::Post => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::After | CicsPlanOption::At
        ),
        CicsPlanOperation::WriteOperator => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::OperatorImmediate
                | CicsPlanOption::OperatorEventual
                | CicsPlanOption::OperatorCritical
        ),
        CicsPlanOperation::ExtractCertificate => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::CertificateOwner
                | CicsPlanOption::CertificateIssuer
        ),
        CicsPlanOperation::AllocateConversation | CicsPlanOperation::GdsAllocateConversation => {
            !matches!(
                option,
                CicsPlanOption::NoHandle | CicsPlanOption::ConversationNoQueue
            )
        }
        CicsPlanOperation::Converse => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::ConversationNotruncate
                | CicsPlanOption::ConversationDefresp
                | CicsPlanOption::ConversationFmh
        ),
        CicsPlanOperation::FormatTime => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::DateSep | CicsPlanOption::TimeSep
        ),
        CicsPlanOperation::Start => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::Fmh
                | CicsPlanOption::Protect
                | CicsPlanOption::After
                | CicsPlanOption::At
                | CicsPlanOption::NoCheck
        ),
        CicsPlanOperation::Delay => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::For | CicsPlanOption::Until
        ),
        CicsPlanOperation::Retrieve => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Wait)
        }
        CicsPlanOperation::ReceiveMap => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Terminal)
        }
        CicsPlanOperation::InvokeApplication => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::ExactMatch | CicsPlanOption::Minimum
        ),
        CicsPlanOperation::Load => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Hold)
        }
        CicsPlanOperation::SpoolClose => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::SpoolKeep | CicsPlanOption::SpoolDelete
        ),
        CicsPlanOperation::SpoolOpenInput => !matches!(option, CicsPlanOption::NoHandle),
        CicsPlanOperation::SpoolOpenOutput => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::SpoolNoCc
                | CicsPlanOption::SpoolAsa
                | CicsPlanOption::SpoolMcc
                | CicsPlanOption::SpoolPrint
                | CicsPlanOption::SpoolPunch
        ),
        CicsPlanOperation::SpoolRead => !matches!(option, CicsPlanOption::NoHandle),
        CicsPlanOperation::SpoolWrite => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::SpoolLine | CicsPlanOption::SpoolPage
        ),
        CicsPlanOperation::DefineCounter
        | CicsPlanOperation::DefineDCounter
        | CicsPlanOperation::DeleteCounter
        | CicsPlanOperation::DeleteDCounter
        | CicsPlanOperation::QueryCounter
        | CicsPlanOperation::QueryDCounter
        | CicsPlanOperation::RewindCounter
        | CicsPlanOperation::RewindDCounter
        | CicsPlanOperation::UpdateCounter
        | CicsPlanOperation::UpdateDCounter => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::CounterNoSuspend
        ),
        CicsPlanOperation::GetCounter | CicsPlanOperation::GetDCounter => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::CounterNoSuspend
                | CicsPlanOption::CounterReduce
                | CicsPlanOption::CounterWrap
        ),
        CicsPlanOperation::EnterTraceNum => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::TraceException
        ),
        CicsPlanOperation::Monitor => !matches!(option, CicsPlanOption::NoHandle),
        CicsPlanOperation::DumpTransaction => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::DumpComplete
                | CicsPlanOption::DumpTask
                | CicsPlanOption::DumpStorage
                | CicsPlanOption::DumpProgram
                | CicsPlanOption::DumpTerminal
                | CicsPlanOption::DumpTables
                | CicsPlanOption::DumpFct
                | CicsPlanOption::DumpPct
                | CicsPlanOption::DumpPpt
                | CicsPlanOption::DumpSit
                | CicsPlanOption::DumpTct
                | CicsPlanOption::DumpTrt
        ),
        CicsPlanOperation::Dump => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::DumpComplete
                | CicsPlanOption::DumpTask
                | CicsPlanOption::DumpStorage
                | CicsPlanOption::DumpProgram
                | CicsPlanOption::DumpTerminal
                | CicsPlanOption::DumpTables
                | CicsPlanOption::DumpDct
                | CicsPlanOption::DumpFct
                | CicsPlanOption::DumpPct
                | CicsPlanOption::DumpPpt
                | CicsPlanOption::DumpSit
                | CicsPlanOption::DumpTct
        ),
        CicsPlanOperation::Trace => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::TraceOn
                | CicsPlanOption::TraceOff
                | CicsPlanOption::TraceSystem
                | CicsPlanOption::TraceUser
                | CicsPlanOption::TraceEi
                | CicsPlanOption::TraceSingle
        ),
        CicsPlanOperation::EnterTraceId => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::TraceAccount
                | CicsPlanOption::TraceMonitor
                | CicsPlanOption::TracePerform
        ),
        CicsPlanOperation::Read => !matches!(
            option,
            CicsPlanOption::Generic
                | CicsPlanOption::Gteq
                | CicsPlanOption::Equal
                | CicsPlanOption::NoHandle
                | CicsPlanOption::Update
        ),
        CicsPlanOperation::DocumentCreate => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Unescaped)
        }
        CicsPlanOperation::DocumentRetrieve => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::DocumentDataOnly
        ),
        CicsPlanOperation::DocumentSet => {
            !matches!(option, CicsPlanOption::NoHandle | CicsPlanOption::Unescaped)
        }
        CicsPlanOperation::DefineCompositeEvent => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::EventAnd | CicsPlanOption::EventOr
        ),
        CicsPlanOperation::DefineTimer => !matches!(
            option,
            CicsPlanOption::NoHandle
                | CicsPlanOption::TimerAfter
                | CicsPlanOption::TimerAt
                | CicsPlanOption::TimerOn
        ),
        CicsPlanOperation::ForceTimer => !matches!(
            option,
            CicsPlanOption::NoHandle | CicsPlanOption::AcqActivity | CicsPlanOption::AcqProcess
        ),
        CicsPlanOperation::TransformDataToJson
        | CicsPlanOperation::TransformDataToXml
        | CicsPlanOperation::TransformJsonToData
        | CicsPlanOperation::TransformXmlToData => !matches!(option, CicsPlanOption::NoHandle),
        _ => !matches!(option, CicsPlanOption::NoHandle),
    })
}
