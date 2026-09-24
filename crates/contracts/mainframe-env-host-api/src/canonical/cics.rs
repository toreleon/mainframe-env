//! Frozen explicit CICS host wire schema.

use super::*;

impl Canonical for CicsOperation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::ExtractAttach => out.variant("CicsOperation", "ExtractAttach", 0),
            Self::ExtractAttributes => out.variant("CicsOperation", "ExtractAttributes", 0),
            Self::GdsExtractAttributes => out.variant("CicsOperation", "GdsExtractAttributes", 0),
            Self::ExtractLogonMsg => out.variant("CicsOperation", "ExtractLogonMsg", 0),
            Self::ExtractProcess => out.variant("CicsOperation", "ExtractProcess", 0),
            Self::GdsExtractProcess => out.variant("CicsOperation", "GdsExtractProcess", 0),
            Self::ExtractTct => out.variant("CicsOperation", "ExtractTct", 0),
            Self::Point => out.variant("CicsOperation", "Point", 0),
            Self::AllocateConversation => out.variant("CicsOperation", "AllocateConversation", 0),
            Self::GdsAllocateConversation => {
                out.variant("CicsOperation", "GdsAllocateConversation", 0)
            }
            Self::GdsAssignConversation => out.variant("CicsOperation", "GdsAssignConversation", 0),
            Self::BuildAttach => out.variant("CicsOperation", "BuildAttach", 0),
            Self::ConnectProcess => out.variant("CicsOperation", "ConnectProcess", 0),
            Self::GdsConnectProcess => out.variant("CicsOperation", "GdsConnectProcess", 0),
            Self::FreeConversation => out.variant("CicsOperation", "FreeConversation", 0),
            Self::GdsFreeConversation => out.variant("CicsOperation", "GdsFreeConversation", 0),
            Self::Converse => out.variant("CicsOperation", "Converse", 0),
            Self::ReceiveConversation => out.variant("CicsOperation", "ReceiveConversation", 0),
            Self::GdsReceiveConversation => {
                out.variant("CicsOperation", "GdsReceiveConversation", 0)
            }
            Self::SendConversation => out.variant("CicsOperation", "SendConversation", 0),
            Self::GdsWaitConversation => out.variant("CicsOperation", "GdsWaitConversation", 0),
            Self::WaitConvid => out.variant("CicsOperation", "WaitConvid", 0),
            Self::WaitTerminal => out.variant("CicsOperation", "WaitTerminal", 0),
            Self::Abend => out.variant("CicsOperation", "Abend", 0),
            Self::FetchAny => out.variant("CicsOperation", "FetchAny", 0),
            Self::FetchChild => out.variant("CicsOperation", "FetchChild", 0),
            Self::FreeChild => out.variant("CicsOperation", "FreeChild", 0),
            Self::LinkAcqActivity => out.variant("CicsOperation", "LinkAcqActivity", 0),
            Self::LinkAcqProcess => out.variant("CicsOperation", "LinkAcqProcess", 0),
            Self::LinkActivity => out.variant("CicsOperation", "LinkActivity", 0),
            Self::AddSubevent => out.variant("CicsOperation", "AddSubevent", 0),
            Self::Address => out.variant("CicsOperation", "Address", 0),
            Self::AddressSet => out.variant("CicsOperation", "AddressSet", 0),
            Self::Asktime => out.variant("CicsOperation", "Asktime", 0),
            Self::BifDeedit => out.variant("CicsOperation", "BifDeedit", 0),
            Self::BifDigest => out.variant("CicsOperation", "BifDigest", 0),
            Self::AsktimeEib => out.variant("CicsOperation", "AsktimeEib", 0),
            Self::Assign => out.variant("CicsOperation", "Assign", 0),
            Self::Cancel => out.variant("CicsOperation", "Cancel", 0),
            Self::ChangePassword => out.variant("CicsOperation", "ChangePassword", 0),
            Self::ChangePhrase => out.variant("CicsOperation", "ChangePhrase", 0),
            Self::ChangeTask => out.variant("CicsOperation", "ChangeTask", 0),
            Self::Delay => out.variant("CicsOperation", "Delay", 0),
            Self::DefineCounter => out.variant("CicsOperation", "DefineCounter", 0),
            Self::DefineDCounter => out.variant("CicsOperation", "DefineDCounter", 0),
            Self::DeleteCounter => out.variant("CicsOperation", "DeleteCounter", 0),
            Self::DeleteDCounter => out.variant("CicsOperation", "DeleteDCounter", 0),
            Self::GetCounter => out.variant("CicsOperation", "GetCounter", 0),
            Self::GetDCounter => out.variant("CicsOperation", "GetDCounter", 0),
            Self::QueryCounter => out.variant("CicsOperation", "QueryCounter", 0),
            Self::QueryDCounter => out.variant("CicsOperation", "QueryDCounter", 0),
            Self::RewindCounter => out.variant("CicsOperation", "RewindCounter", 0),
            Self::RewindDCounter => out.variant("CicsOperation", "RewindDCounter", 0),
            Self::UpdateCounter => out.variant("CicsOperation", "UpdateCounter", 0),
            Self::UpdateDCounter => out.variant("CicsOperation", "UpdateDCounter", 0),
            Self::Post => out.variant("CicsOperation", "Post", 0),
            Self::WriteOperator => out.variant("CicsOperation", "WriteOperator", 0),
            Self::ExtractCertificate => out.variant("CicsOperation", "ExtractCertificate", 0),
            Self::ExtractTcpip => out.variant("CicsOperation", "ExtractTcpip", 0),
            Self::Deq => out.variant("CicsOperation", "Deq", 0),
            Self::Delete => out.variant("CicsOperation", "Delete", 0),
            Self::DefineInputEvent => out.variant("CicsOperation", "DefineInputEvent", 0),
            Self::DefineCompositeEvent => out.variant("CicsOperation", "DefineCompositeEvent", 0),
            Self::DeleteEvent => out.variant("CicsOperation", "DeleteEvent", 0),
            Self::CheckTimer => out.variant("CicsOperation", "CheckTimer", 0),
            Self::DefineTimer => out.variant("CicsOperation", "DefineTimer", 0),
            Self::DeleteTimer => out.variant("CicsOperation", "DeleteTimer", 0),
            Self::RetrieveReattachEvent => out.variant("CicsOperation", "RetrieveReattachEvent", 0),
            Self::RetrieveSubevent => out.variant("CicsOperation", "RetrieveSubevent", 0),
            Self::TestEvent => out.variant("CicsOperation", "TestEvent", 0),
            Self::SignalEvent => out.variant("CicsOperation", "SignalEvent", 0),
            Self::ForceTimer => out.variant("CicsOperation", "ForceTimer", 0),
            Self::DocumentCreate => out.variant("CicsOperation", "DocumentCreate", 0),
            Self::DocumentDelete => out.variant("CicsOperation", "DocumentDelete", 0),
            Self::DocumentInsert => out.variant("CicsOperation", "DocumentInsert", 0),
            Self::DocumentRetrieve => out.variant("CicsOperation", "DocumentRetrieve", 0),
            Self::DocumentSet => out.variant("CicsOperation", "DocumentSet", 0),
            Self::DeleteTransientData => out.variant("CicsOperation", "DeleteTransientData", 0),
            Self::DeleteTemporaryStorage => {
                out.variant("CicsOperation", "DeleteTemporaryStorage", 0)
            }
            Self::ReadTemporaryStorage => out.variant("CicsOperation", "ReadTemporaryStorage", 0),
            Self::WriteTemporaryStorage => out.variant("CicsOperation", "WriteTemporaryStorage", 0),
            Self::Enq => out.variant("CicsOperation", "Enq", 0),
            Self::EndBrowse => out.variant("CicsOperation", "EndBrowse", 0),
            Self::FormatTime => out.variant("CicsOperation", "FormatTime", 0),
            Self::ConvertTime => out.variant("CicsOperation", "ConvertTime", 0),
            Self::Freemain => out.variant("CicsOperation", "Freemain", 0),
            Self::Freemain64 => out.variant("CicsOperation", "Freemain64", 0),
            Self::Getmain => out.variant("CicsOperation", "Getmain", 0),
            Self::Getmain64 => out.variant("CicsOperation", "Getmain64", 0),
            Self::HandleAbend => out.variant("CicsOperation", "HandleAbend", 0),
            Self::HandleAid => out.variant("CicsOperation", "HandleAid", 0),
            Self::HandleCondition => out.variant("CicsOperation", "HandleCondition", 0),
            Self::IgnoreCondition => out.variant("CicsOperation", "IgnoreCondition", 0),
            Self::Inquire => out.variant("CicsOperation", "Inquire", 0),
            Self::InvokeApplication => out.variant("CicsOperation", "InvokeApplication", 0),
            Self::Load => out.variant("CicsOperation", "Load", 0),
            Self::Release => out.variant("CicsOperation", "Release", 0),
            Self::Link => out.variant("CicsOperation", "Link", 0),
            Self::PopHandle => out.variant("CicsOperation", "PopHandle", 0),
            Self::PushHandle => out.variant("CicsOperation", "PushHandle", 0),
            Self::PurgeMessage => out.variant("CicsOperation", "PurgeMessage", 0),
            Self::QuerySecurity => out.variant("CicsOperation", "QuerySecurity", 0),
            Self::RequestPassTicket => out.variant("CicsOperation", "RequestPassTicket", 0),
            Self::RequestEncryptPassTicket => {
                out.variant("CicsOperation", "RequestEncryptPassTicket", 0)
            }
            Self::Signon => out.variant("CicsOperation", "Signon", 0),
            Self::Signoff => out.variant("CicsOperation", "Signoff", 0),
            Self::Read => out.variant("CicsOperation", "Read", 0),
            Self::ReadNext => out.variant("CicsOperation", "ReadNext", 0),
            Self::ReadPrev => out.variant("CicsOperation", "ReadPrev", 0),
            Self::ResetBrowse => out.variant("CicsOperation", "ResetBrowse", 0),
            Self::ReadTransientData => out.variant("CicsOperation", "ReadTransientData", 0),
            Self::RemoveSubevent => out.variant("CicsOperation", "RemoveSubevent", 0),
            Self::ReceiveMap => out.variant("CicsOperation", "ReceiveMap", 0),
            Self::Retrieve => out.variant("CicsOperation", "Retrieve", 0),
            Self::Return => out.variant("CicsOperation", "Return", 0),
            Self::Rewrite => out.variant("CicsOperation", "Rewrite", 0),
            Self::SendText => out.variant("CicsOperation", "SendText", 0),
            Self::SendMap => out.variant("CicsOperation", "SendMap", 0),
            Self::SendPartnset => out.variant("CicsOperation", "SendPartnset", 0),
            Self::SendControl => out.variant("CicsOperation", "SendControl", 0),
            Self::IssueAbort => out.variant("CicsOperation", "IssueAbort", 0),
            Self::IssueAdd => out.variant("CicsOperation", "IssueAdd", 0),
            Self::IssueEnd => out.variant("CicsOperation", "IssueEnd", 0),
            Self::IssueErase => out.variant("CicsOperation", "IssueErase", 0),
            Self::IssueNote => out.variant("CicsOperation", "IssueNote", 0),
            Self::IssueQuery => out.variant("CicsOperation", "IssueQuery", 0),
            Self::IssueReceive => out.variant("CicsOperation", "IssueReceive", 0),
            Self::IssueReplace => out.variant("CicsOperation", "IssueReplace", 0),
            Self::IssueSend => out.variant("CicsOperation", "IssueSend", 0),
            Self::Route => out.variant("CicsOperation", "Route", 0),
            Self::IssueWait => out.variant("CicsOperation", "IssueWait", 0),
            Self::SendPage => out.variant("CicsOperation", "SendPage", 0),
            Self::ReceivePartn => out.variant("CicsOperation", "ReceivePartn", 0),
            Self::SetAssociationUserCorrData => {
                out.variant("CicsOperation", "SetAssociationUserCorrData", 0)
            }
            Self::SetFileStatus => out.variant("CicsOperation", "SetFileStatus", 0),
            Self::SpoolClose => out.variant("CicsOperation", "SpoolClose", 0),
            Self::SpoolOpenInput => out.variant("CicsOperation", "SpoolOpenInput", 0),
            Self::SpoolOpenOutput => out.variant("CicsOperation", "SpoolOpenOutput", 0),
            Self::SpoolRead => out.variant("CicsOperation", "SpoolRead", 0),
            Self::SpoolWrite => out.variant("CicsOperation", "SpoolWrite", 0),
            Self::EnterTraceNum => out.variant("CicsOperation", "EnterTraceNum", 0),
            Self::Monitor => out.variant("CicsOperation", "Monitor", 0),
            Self::DumpTransaction => out.variant("CicsOperation", "DumpTransaction", 0),
            Self::Dump => out.variant("CicsOperation", "Dump", 0),
            Self::Trace => out.variant("CicsOperation", "Trace", 0),
            Self::EnterTraceId => out.variant("CicsOperation", "EnterTraceId", 0),
            Self::Start => out.variant("CicsOperation", "Start", 0),
            Self::StartAttach => out.variant("CicsOperation", "StartAttach", 0),
            Self::StartBrexit => out.variant("CicsOperation", "StartBrexit", 0),
            Self::StartBrowse => out.variant("CicsOperation", "StartBrowse", 0),
            Self::Suspend => out.variant("CicsOperation", "Suspend", 0),
            Self::WaitEvent => out.variant("CicsOperation", "WaitEvent", 0),
            Self::WaitExternal => out.variant("CicsOperation", "WaitExternal", 0),
            Self::WaitCics => out.variant("CicsOperation", "WaitCics", 0),
            Self::WaitSignal => out.variant("CicsOperation", "WaitSignal", 0),
            Self::Syncpoint => out.variant("CicsOperation", "Syncpoint", 0),
            Self::InvokeService => out.variant("CicsOperation", "InvokeService", 0),
            Self::SoapFaultAdd => out.variant("CicsOperation", "SoapFaultAdd", 0),
            Self::SoapFaultCreate => out.variant("CicsOperation", "SoapFaultCreate", 0),
            Self::SoapFaultDelete => out.variant("CicsOperation", "SoapFaultDelete", 0),
            Self::WsaContextBuild => out.variant("CicsOperation", "WsaContextBuild", 0),
            Self::WsaContextDelete => out.variant("CicsOperation", "WsaContextDelete", 0),
            Self::WsaContextGet => out.variant("CicsOperation", "WsaContextGet", 0),
            Self::WsaEprCreate => out.variant("CicsOperation", "WsaEprCreate", 0),
            Self::TransformDataToJson => out.variant("CicsOperation", "TransformDataToJson", 0),
            Self::TransformDataToXml => out.variant("CicsOperation", "TransformDataToXml", 0),
            Self::TransformJsonToData => out.variant("CicsOperation", "TransformJsonToData", 0),
            Self::TransformXmlToData => out.variant("CicsOperation", "TransformXmlToData", 0),
            Self::WebParseUrl => out.variant("CicsOperation", "WebParseUrl", 0),
            Self::WebOpen => out.variant("CicsOperation", "WebOpen", 0),
            Self::WebClose => out.variant("CicsOperation", "WebClose", 0),
            Self::WebExtract => out.variant("CicsOperation", "WebExtract", 0),
            Self::ExtractWeb => out.variant("CicsOperation", "ExtractWeb", 0),
            Self::WebRead => out.variant("CicsOperation", "WebRead", 0),
            Self::WebStartBrowse => out.variant("CicsOperation", "WebStartBrowse", 0),
            Self::WebReadNext => out.variant("CicsOperation", "WebReadNext", 0),
            Self::WebEndBrowse => out.variant("CicsOperation", "WebEndBrowse", 0),
            Self::WebWrite => out.variant("CicsOperation", "WebWrite", 0),
            Self::WebSend => out.variant("CicsOperation", "WebSend", 0),
            Self::WebRetrieve => out.variant("CicsOperation", "WebRetrieve", 0),
            Self::WebReceive => out.variant("CicsOperation", "WebReceive", 0),
            Self::WebConverse => out.variant("CicsOperation", "WebConverse", 0),
            Self::WaitJournalName => out.variant("CicsOperation", "WaitJournalName", 0),
            Self::WaitJournalNum => out.variant("CicsOperation", "WaitJournalNum", 0),
            Self::WriteJournalName => out.variant("CicsOperation", "WriteJournalName", 0),
            Self::WriteJournalNum => out.variant("CicsOperation", "WriteJournalNum", 0),
            Self::Unlock => out.variant("CicsOperation", "Unlock", 0),
            Self::VerifyPassword => out.variant("CicsOperation", "VerifyPassword", 0),
            Self::VerifyPhrase => out.variant("CicsOperation", "VerifyPhrase", 0),
            Self::VerifyToken => out.variant("CicsOperation", "VerifyToken", 0),
            Self::Write => out.variant("CicsOperation", "Write", 0),
            Self::WriteTransientData => out.variant("CicsOperation", "WriteTransientData", 0),
            Self::Xctl => out.variant("CicsOperation", "Xctl", 0),
        }
    }
}

impl Canonical for CicsConditionPolicy {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Default => out.variant("CicsConditionPolicy", "Default", 0),
            Self::NoHandle => out.variant("CicsConditionPolicy", "NoHandle", 0),
            Self::Respond {
                response2_field,
                response_field,
            } => {
                out.variant("CicsConditionPolicy", "Respond", 2)?;
                out.text("response2_field")?;
                response2_field.encode(out)?;
                out.text("response_field")?;
                response_field.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for CicsRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            arguments,
            condition_policy,
            mutation,
            operation,
        } = self;
        out.object("CicsRequest", 4)?;
        out.text("arguments")?;
        arguments.encode(out)?;
        out.text("condition_policy")?;
        condition_policy.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("operation")?;
        operation.encode(out)?;
        Ok(())
    }
}

impl Canonical for CicsDisposition {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Complete => out.variant("CicsDisposition", "Complete", 0),
            Self::Ignored => out.variant("CicsDisposition", "Ignored", 0),
            Self::Suspended => out.variant("CicsDisposition", "Suspended", 0),
            Self::Transfer => out.variant("CicsDisposition", "Transfer", 0),
            Self::Handler => out.variant("CicsDisposition", "Handler", 0),
            Self::Returned => out.variant("CicsDisposition", "Returned", 0),
            Self::Abended => out.variant("CicsDisposition", "Abended", 0),
        }
    }
}

impl Canonical for CicsUnitOfWorkOutcome {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Committed => out.variant("CicsUnitOfWorkOutcome", "Committed", 0),
            Self::RolledBack => out.variant("CicsUnitOfWorkOutcome", "RolledBack", 0),
        }
    }
}

impl Canonical for CicsResponse {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            aid,
            applid,
            condition,
            disposition,
            next_transaction,
            outputs,
            payload,
            response,
            response2,
            sysid,
            target,
            transaction,
            unit_of_work,
        } = self;
        out.object("CicsResponse", 13)?;
        out.text("aid")?;
        aid.encode(out)?;
        out.text("applid")?;
        applid.encode(out)?;
        out.text("condition")?;
        condition.encode(out)?;
        out.text("disposition")?;
        disposition.encode(out)?;
        out.text("next_transaction")?;
        next_transaction.encode(out)?;
        out.text("outputs")?;
        outputs.encode(out)?;
        out.text("payload")?;
        payload.encode(out)?;
        out.text("response")?;
        response.encode(out)?;
        out.text("response2")?;
        response2.encode(out)?;
        out.text("sysid")?;
        sysid.encode(out)?;
        out.text("target")?;
        target.encode(out)?;
        out.text("transaction")?;
        transaction.encode(out)?;
        out.text("unit_of_work")?;
        unit_of_work.encode(out)?;
        Ok(())
    }
}
