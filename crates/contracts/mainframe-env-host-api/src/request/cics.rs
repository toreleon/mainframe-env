//! Typed CICS host request and result boundary.

use super::{HostLimits, HostProblem, Mutation};
use mainframe_env_execution_api::BoundedPayload;
use std::collections::BTreeMap;

/// Typed CICS operations admitted at the host request boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsOperation {
    Abend,
    /// Add one atomic event to an activity-owned composite predicate.
    AddSubevent,
    /// Return checked virtual addresses for task storage areas.
    Address,
    /// Copy one checked virtual pointer/address relationship.
    AddressSet,
    Asktime,
    /// Refresh only the implicit EIB date and time fields.
    AsktimeEib,
    Assign,
    /// Cancel one unhonored local interval-control START request.
    Cancel,
    /// Change the issuing CICS task's dispatch priority.
    ChangeTask,
    /// Complete the source-defined zero-delay interval-control boundary.
    Delay,
    /// Define a named signed fullword counter in a selected pool.
    DefineCounter,
    /// Define a named unsigned doubleword counter in a selected pool.
    DefineDCounter,
    /// Execute IBM DELETE against a signed fullword named counter.
    DeleteCounter,
    /// Execute IBM DELETE against an unsigned doubleword named counter.
    DeleteDCounter,
    /// Execute IBM GET against a signed fullword named counter.
    GetCounter,
    /// Execute IBM GET against an unsigned doubleword named counter.
    GetDCounter,
    /// Execute IBM QUERY against a signed fullword named counter.
    QueryCounter,
    /// Execute IBM QUERY against an unsigned doubleword named counter.
    QueryDCounter,
    /// Execute IBM REWIND against a signed fullword named counter.
    RewindCounter,
    /// Execute IBM REWIND against an unsigned doubleword named counter.
    RewindDCounter,
    /// Execute IBM UPDATE against a signed fullword named counter.
    UpdateCounter,
    /// Execute IBM UPDATE against an unsigned doubleword named counter.
    UpdateDCounter,
    /// Release one matching task enqueue ownership level.
    Deq,
    /// Delete the current file record.
    Delete,
    /// Define one activity-owned BTS input event.
    DefineInputEvent,
    /// Define an AND or OR predicate over activity-owned atomic events.
    DefineCompositeEvent,
    /// Delete one input or composite event from the current activity.
    DeleteEvent,
    /// BTS timer state command.
    CheckTimer,
    /// BTS timer state command.
    DefineTimer,
    /// BTS timer state command.
    DeleteTimer,
    /// BTS event retrieval or status command.
    RetrieveReattachEvent,
    /// BTS event retrieval or status command.
    RetrieveSubevent,
    /// BTS event retrieval or status command.
    TestEvent,
    /// Emit matching business events from an application capture point.
    SignalEvent,
    /// BTS timer state command.
    ForceTimer,
    /// Create one bounded transaction-owned document.
    DocumentCreate,
    /// Delete one transaction-owned document and release its storage.
    DocumentDelete,
    /// Insert content or bookmarks into one transaction-owned document.
    DocumentInsert,
    /// Copy one transaction-owned document into an application buffer.
    DocumentRetrieve,
    /// Add or replace symbols in one transaction-owned document.
    DocumentSet,
    /// Delete every record from one local transient-data queue.
    DeleteTransientData,
    /// Delete every item from one local temporary-storage queue.
    DeleteTemporaryStorage,
    /// Read one item from one local temporary-storage queue.
    ReadTemporaryStorage,
    /// Append or replace one item in one local temporary-storage queue.
    WriteTemporaryStorage,
    /// Acquire or wait for one task enqueue resource.
    Enq,
    EndBrowse,
    FormatTime,
    /// Release one task-local virtual storage area acquired by GETMAIN.
    Freemain,
    /// Release one checked AMODE(64) virtual allocation.
    Freemain64,
    /// Allocate one bounded task-local virtual storage area.
    Getmain,
    /// Admit a checked non-LE AMODE(64) virtual allocation request.
    Getmain64,
    HandleAbend,
    /// Install or deactivate one bounded set of terminal AID handlers.
    HandleAid,
    /// Install or deactivate one bounded set of reviewed condition handlers.
    HandleCondition,
    /// Ignore one bounded set of reviewed EIBRESP conditions for this program level.
    IgnoreCondition,
    Inquire,
    /// Select and invoke one installed application operation.
    InvokeApplication,
    /// Load one immutable installed program generation for the issuing task.
    Load,
    /// Release one prior program LOAD ownership level.
    Release,
    Link,
    /// Restore one suspended HANDLE/IGNORE specification snapshot.
    PopHandle,
    /// Suspend the current HANDLE/IGNORE specifications in one nested snapshot.
    PushHandle,
    /// Discard the current full-BMS logical message, if one is being built.
    PurgeMessage,
    Read,
    ReadNext,
    ReadPrev,
    /// Reposition an active file browse without replacing its cursor.
    ResetBrowse,
    /// Read and consume one record from a local transient-data queue.
    ReadTransientData,
    /// Remove one atomic child without deleting or resetting it.
    RemoveSubevent,
    ReceiveMap,
    /// Receive one 8775 partition input message and identify its partition.
    ReceivePartn,
    Retrieve,
    Return,
    Rewrite,
    SendText,
    SendMap,
    /// Send BMS device controls, directly or into a logical message.
    SendControl,
    /// Abort and deselect one outboard stream.
    IssueAbort,
    /// Append or place bounded records in an outboard data set.
    IssueAdd,
    /// End and deselect one outboard stream.
    IssueEnd,
    /// Erase selected records in a direct outboard data set.
    IssueErase,
    /// Return the next relative record number.
    IssueNote,
    /// Request a sequential outboard input stream.
    IssueQuery,
    /// Consume one record from an outboard input stream.
    IssueReceive,
    /// Replace selected direct outboard records.
    IssueReplace,
    /// Transmit one bounded outboard record or media message.
    IssueSend,
    /// Route one full-BMS logical message to eligible terminal recipients.
    Route,
    /// Complete one pending outboard send.
    IssueWait,
    /// Complete and dispatch the active BMS logical message.
    SendPage,
    /// Associate a registered partition set or return the terminal to base state.
    SendPartnset,
    /// Overwrite the originating task's bounded user correlator data.
    SetAssociationUserCorrData,
    SetFileStatus,
    /// Close one task-owned CICS spool report.
    SpoolClose,
    /// Open one matching CICS spool report for input.
    SpoolOpenInput,
    /// Create one CICS spool report for output.
    SpoolOpenOutput,
    /// Read the next record of one open input spool report.
    SpoolRead,
    /// Append one record to an open output spool report.
    SpoolWrite,
    /// Write a bounded user trace entry by numeric trace identifier.
    EnterTraceNum,
    /// Record a configured user event monitoring point.
    Monitor,
    /// Capture a bounded local transaction diagnostic dump.
    DumpTransaction,
    /// Capture a bounded local CICS diagnostic dump.
    Dump,
    /// Change the bounded local diagnostic trace switches.
    Trace,
    /// Retain a bounded named user trace and local monitoring event.
    EnterTraceId,
    /// Schedule one interval-control START record.
    Start,
    StartBrowse,
    /// Relinquish control until the task is redispatched.
    Suspend,
    /// Wait for one timer-event control area to be posted.
    WaitEvent,
    /// Wait for standard MVS posting of one ECB in a bounded external list.
    WaitExternal,
    Syncpoint,
    /// Typed CICS web-service-control command InvokeService.
    InvokeService,
    /// Typed CICS web-service-control command SoapFaultAdd.
    SoapFaultAdd,
    /// Typed CICS web-service-control command SoapFaultCreate.
    SoapFaultCreate,
    /// Typed CICS web-service-control command SoapFaultDelete.
    SoapFaultDelete,
    /// Typed CICS web-service-control command WsaContextBuild.
    WsaContextBuild,
    /// Typed CICS web-service-control command WsaContextDelete.
    WsaContextDelete,
    /// Typed CICS web-service-control command WsaContextGet.
    WsaContextGet,
    /// Typed CICS web-service-control command WsaEprCreate.
    WsaEprCreate,
    /// Convert one BIT-mode application-data container to canonical JSON.
    TransformDataToJson,
    /// Convert one BIT-mode application-data container to deterministic XML.
    TransformDataToXml,
    /// Convert JSON from a channel container to application data.
    TransformJsonToData,
    /// Query XML metadata or convert an XML container to application data.
    TransformXmlToData,
    /// Split a bounded URL into its scheme, host, port, path, and query components.
    WebParseUrl,
    /// Synchronize this task with output for one named journal.
    WaitJournalName,
    /// Synchronize this task with output for one numbered journal.
    WaitJournalNum,
    /// Create one named journal record with synchronous or deferred output.
    WriteJournalName,
    /// Create one numbered journal record with synchronous or deferred output.
    WriteJournalNum,
    /// Invalidate one task-owned file update context.
    Unlock,
    Write,
    WriteTransientData,
    Xctl,
}

impl CicsOperation {
    /// Stable runtime-registration spelling for this host operation.
    #[must_use]
    pub const fn runtime_name(self) -> &'static str {
        match self {
            Self::Abend => "Abend",
            Self::AddSubevent => "AddSubevent",
            Self::Address => "Address",
            Self::AddressSet => "AddressSet",
            Self::Asktime => "Asktime",
            Self::AsktimeEib => "AsktimeEib",
            Self::Assign => "Assign",
            Self::Cancel => "Cancel",
            Self::ChangeTask => "ChangeTask",
            Self::Delay => "Delay",
            Self::DefineCounter => "DefineCounter",
            Self::DefineDCounter => "DefineDCounter",
            Self::DeleteCounter => "DeleteCounter",
            Self::DeleteDCounter => "DeleteDCounter",
            Self::GetCounter => "GetCounter",
            Self::GetDCounter => "GetDCounter",
            Self::QueryCounter => "QueryCounter",
            Self::QueryDCounter => "QueryDCounter",
            Self::RewindCounter => "RewindCounter",
            Self::RewindDCounter => "RewindDCounter",
            Self::UpdateCounter => "UpdateCounter",
            Self::UpdateDCounter => "UpdateDCounter",
            Self::Deq => "Deq",
            Self::Delete => "Delete",
            Self::DefineInputEvent => "DefineInputEvent",
            Self::DefineCompositeEvent => "DefineCompositeEvent",
            Self::DeleteEvent => "DeleteEvent",
            Self::CheckTimer => "CheckTimer",
            Self::DefineTimer => "DefineTimer",
            Self::DeleteTimer => "DeleteTimer",
            Self::RetrieveReattachEvent => "RetrieveReattachEvent",
            Self::RetrieveSubevent => "RetrieveSubevent",
            Self::TestEvent => "TestEvent",
            Self::SignalEvent => "SignalEvent",
            Self::ForceTimer => "ForceTimer",
            Self::DocumentCreate => "DocumentCreate",
            Self::DocumentDelete => "DocumentDelete",
            Self::DocumentInsert => "DocumentInsert",
            Self::DocumentRetrieve => "DocumentRetrieve",
            Self::DocumentSet => "DocumentSet",
            Self::DeleteTransientData => "DeleteTransientData",
            Self::DeleteTemporaryStorage => "DeleteTemporaryStorage",
            Self::ReadTemporaryStorage => "ReadTemporaryStorage",
            Self::WriteTemporaryStorage => "WriteTemporaryStorage",
            Self::Enq => "Enq",
            Self::EndBrowse => "EndBrowse",
            Self::FormatTime => "FormatTime",
            Self::Freemain => "Freemain",
            Self::Freemain64 => "Freemain64",
            Self::Getmain => "Getmain",
            Self::Getmain64 => "Getmain64",
            Self::HandleAbend => "HandleAbend",
            Self::HandleAid => "HandleAid",
            Self::HandleCondition => "HandleCondition",
            Self::IgnoreCondition => "IgnoreCondition",
            Self::Inquire => "Inquire",
            Self::InvokeApplication => "InvokeApplication",
            Self::Load => "Load",
            Self::Release => "Release",
            Self::Link => "Link",
            Self::PopHandle => "PopHandle",
            Self::PushHandle => "PushHandle",
            Self::PurgeMessage => "PurgeMessage",
            Self::Read => "Read",
            Self::ReadNext => "ReadNext",
            Self::ReadPrev => "ReadPrev",
            Self::ResetBrowse => "ResetBrowse",
            Self::ReadTransientData => "ReadTransientData",
            Self::RemoveSubevent => "RemoveSubevent",
            Self::ReceiveMap => "ReceiveMap",
            Self::ReceivePartn => "ReceivePartn",
            Self::Retrieve => "Retrieve",
            Self::Return => "Return",
            Self::Rewrite => "Rewrite",
            Self::SendText => "SendText",
            Self::SendMap => "SendMap",
            Self::SendControl => "SendControl",
            Self::IssueAbort => "IssueAbort",
            Self::IssueAdd => "IssueAdd",
            Self::IssueEnd => "IssueEnd",
            Self::IssueErase => "IssueErase",
            Self::IssueNote => "IssueNote",
            Self::IssueQuery => "IssueQuery",
            Self::IssueReceive => "IssueReceive",
            Self::IssueReplace => "IssueReplace",
            Self::IssueSend => "IssueSend",
            Self::Route => "Route",
            Self::IssueWait => "IssueWait",
            Self::SendPage => "SendPage",
            Self::SendPartnset => "SendPartnset",
            Self::SetAssociationUserCorrData => "SetAssociationUserCorrData",
            Self::SetFileStatus => "SetFileStatus",
            Self::SpoolClose => "SpoolClose",
            Self::SpoolOpenInput => "SpoolOpenInput",
            Self::SpoolOpenOutput => "SpoolOpenOutput",
            Self::SpoolRead => "SpoolRead",
            Self::SpoolWrite => "SpoolWrite",
            Self::EnterTraceNum => "EnterTraceNum",
            Self::Monitor => "Monitor",
            Self::DumpTransaction => "DumpTransaction",
            Self::Dump => "Dump",
            Self::Trace => "Trace",
            Self::EnterTraceId => "EnterTraceId",
            Self::Start => "Start",
            Self::StartBrowse => "StartBrowse",
            Self::Suspend => "Suspend",
            Self::WaitEvent => "WaitEvent",
            Self::WaitExternal => "WaitExternal",
            Self::Syncpoint => "Syncpoint",
            Self::InvokeService => "InvokeService",
            Self::SoapFaultAdd => "SoapFaultAdd",
            Self::SoapFaultCreate => "SoapFaultCreate",
            Self::SoapFaultDelete => "SoapFaultDelete",
            Self::WsaContextBuild => "WsaContextBuild",
            Self::WsaContextDelete => "WsaContextDelete",
            Self::WsaContextGet => "WsaContextGet",
            Self::WsaEprCreate => "WsaEprCreate",
            Self::TransformDataToJson => "TransformDataToJson",
            Self::TransformDataToXml => "TransformDataToXml",
            Self::TransformJsonToData => "TransformJsonToData",
            Self::TransformXmlToData => "TransformXmlToData",
            Self::WebParseUrl => "WebParseUrl",
            Self::WaitJournalName => "WaitJournalName",
            Self::WaitJournalNum => "WaitJournalNum",
            Self::WriteJournalName => "WriteJournalName",
            Self::WriteJournalNum => "WriteJournalNum",
            Self::Unlock => "Unlock",
            Self::Write => "Write",
            Self::WriteTransientData => "WriteTransientData",
            Self::Xctl => "Xctl",
        }
    }

    /// Whether this operation belongs to the named-counter family.
    #[must_use]
    pub const fn is_counter(self) -> bool {
        matches!(
            self,
            Self::DefineCounter
                | Self::DefineDCounter
                | Self::DeleteCounter
                | Self::DeleteDCounter
                | Self::GetCounter
                | Self::GetDCounter
                | Self::QueryCounter
                | Self::QueryDCounter
                | Self::RewindCounter
                | Self::RewindDCounter
                | Self::UpdateCounter
                | Self::UpdateDCounter
        )
    }

    /// Whether the operation may change durable or task-local state.
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::Delete
                | Self::AddSubevent
                | Self::DefineInputEvent
                | Self::DefineCompositeEvent
                | Self::DeleteEvent
                | Self::CheckTimer
                | Self::DefineTimer
                | Self::DeleteTimer
                | Self::RetrieveReattachEvent
                | Self::RetrieveSubevent
                | Self::TestEvent
                | Self::SignalEvent
                | Self::ForceTimer
                | Self::DocumentCreate
                | Self::DocumentDelete
                | Self::DocumentInsert
                | Self::DocumentSet
                | Self::ResetBrowse
                | Self::DeleteTransientData
                | Self::DeleteTemporaryStorage
                | Self::ReadTemporaryStorage
                | Self::WriteTemporaryStorage
                | Self::Cancel
                | Self::Delay
                | Self::DefineCounter
                | Self::DefineDCounter
                | Self::DeleteCounter
                | Self::DeleteDCounter
                | Self::GetCounter
                | Self::GetDCounter
                | Self::RewindCounter
                | Self::RewindDCounter
                | Self::UpdateCounter
                | Self::UpdateDCounter
                | Self::Deq
                | Self::Enq
                | Self::Freemain
                | Self::Freemain64
                | Self::Getmain
                | Self::Getmain64
                | Self::Rewrite
                | Self::Write
                | Self::WriteJournalName
                | Self::WriteJournalNum
                | Self::WriteTransientData
                | Self::Link
                | Self::InvokeApplication
                | Self::InvokeService
                | Self::SoapFaultAdd
                | Self::SoapFaultCreate
                | Self::SoapFaultDelete
                | Self::WsaContextBuild
                | Self::WsaContextDelete
                | Self::WsaEprCreate
                | Self::Load
                | Self::Release
                | Self::ReceiveMap
                | Self::ReceivePartn
                | Self::ReadTransientData
                | Self::RemoveSubevent
                | Self::PurgeMessage
                | Self::SendMap
                | Self::SendControl
                | Self::IssueAbort
                | Self::IssueAdd
                | Self::IssueEnd
                | Self::IssueErase
                | Self::IssueNote
                | Self::IssueQuery
                | Self::IssueReceive
                | Self::IssueReplace
                | Self::IssueSend
                | Self::Route
                | Self::IssueWait
                | Self::SendPage
                | Self::SendText
                | Self::SendPartnset
                | Self::SetAssociationUserCorrData
                | Self::Xctl
                | Self::Return
                | Self::Abend
                | Self::Syncpoint
                | Self::TransformDataToJson
                | Self::TransformDataToXml
                | Self::TransformJsonToData
                | Self::TransformXmlToData
                | Self::Unlock
                | Self::SetFileStatus
                | Self::SpoolClose
                | Self::SpoolOpenInput
                | Self::SpoolOpenOutput
                | Self::SpoolRead
                | Self::SpoolWrite
                | Self::EnterTraceNum
                | Self::Monitor
                | Self::DumpTransaction
                | Self::Dump
                | Self::Trace
                | Self::EnterTraceId
                | Self::Start
                | Self::Retrieve
                | Self::WaitEvent
                | Self::WaitExternal
        )
    }

    #[must_use]
    pub fn from_tokens(tokens: &[String]) -> Option<Self> {
        let words: Vec<String> = tokens
            .iter()
            .map(|token| token.to_ascii_uppercase())
            .filter(|token| !matches!(token.as_str(), "EXEC" | "CICS" | "END-EXEC"))
            .collect();
        let first = words.first()?.as_str();
        Some(match (first, words.get(1).map(String::as_str)) {
            ("ABEND", _) => Self::Abend,
            ("ADD", Some("SUBEVENT")) => Self::AddSubevent,
            ("ADDRESS", Some("SET")) => Self::AddressSet,
            ("ADDRESS", _) => Self::Address,
            ("ASKTIME", _)
                if words
                    .iter()
                    .any(|word| word == "ABSTIME" || word.starts_with("ABSTIME(")) =>
            {
                Self::Asktime
            }
            ("ASKTIME", _) => Self::AsktimeEib,
            ("ASSIGN", _) => Self::Assign,
            ("CANCEL", _) => Self::Cancel,
            ("CHANGE", Some("TASK")) => Self::ChangeTask,
            ("DELAY", _) => Self::Delay,
            ("DEQ", _) => Self::Deq,
            ("DEFINE", Some("COUNTER")) => Self::DefineCounter,
            ("DEFINE", Some("DCOUNTER")) => Self::DefineDCounter,
            ("DELETE", Some("COUNTER")) => Self::DeleteCounter,
            ("DELETE", Some("DCOUNTER")) => Self::DeleteDCounter,
            ("GET", Some("COUNTER")) => Self::GetCounter,
            ("GET", Some("DCOUNTER")) => Self::GetDCounter,
            ("QUERY", Some("COUNTER")) => Self::QueryCounter,
            ("QUERY", Some("DCOUNTER")) => Self::QueryDCounter,
            ("REWIND", Some("COUNTER")) => Self::RewindCounter,
            ("REWIND", Some("DCOUNTER")) => Self::RewindDCounter,
            ("UPDATE", Some("COUNTER")) => Self::UpdateCounter,
            ("UPDATE", Some("DCOUNTER")) => Self::UpdateDCounter,
            ("DELETE", _) => Self::Delete,
            ("DEFINE", Some("INPUT")) => Self::DefineInputEvent,
            ("DEFINE", Some("COMPOSITE")) => Self::DefineCompositeEvent,
            ("DOCUMENT", Some("CREATE")) => Self::DocumentCreate,
            ("DOCUMENT", Some("DELETE")) => Self::DocumentDelete,
            ("DOCUMENT", Some("INSERT")) => Self::DocumentInsert,
            ("DOCUMENT", Some("RETRIEVE")) => Self::DocumentRetrieve,
            ("DOCUMENT", Some("SET")) => Self::DocumentSet,
            ("DELETEQ", Some("TD")) => Self::DeleteTransientData,
            ("DELETEQ", Some("TS")) => Self::DeleteTemporaryStorage,
            ("READQ", Some("TS")) => Self::ReadTemporaryStorage,
            ("WRITEQ", Some("TS")) => Self::WriteTemporaryStorage,
            ("ENQ", _) => Self::Enq,
            ("ENDBR", _) => Self::EndBrowse,
            ("FORMATTIME", _) => Self::FormatTime,
            ("FREEMAIN", _) => Self::Freemain,
            ("FREEMAIN64", _) => Self::Freemain64,
            ("GETMAIN", _) => Self::Getmain,
            ("GETMAIN64", _) => Self::Getmain64,
            ("HANDLE", Some("ABEND")) => Self::HandleAbend,
            ("HANDLE", Some("AID")) => Self::HandleAid,
            ("HANDLE", Some("CONDITION")) => Self::HandleCondition,
            ("IGNORE", Some("CONDITION")) => Self::IgnoreCondition,
            ("INQUIRE", _) => Self::Inquire,
            ("INVOKE", Some("APPLICATION")) => Self::InvokeApplication,
            ("INVOKE", Some("SERVICE")) => Self::InvokeService,
            ("SOAPFAULT", Some("ADD")) => Self::SoapFaultAdd,
            ("SOAPFAULT", Some("CREATE")) => Self::SoapFaultCreate,
            ("SOAPFAULT", Some("DELETE")) => Self::SoapFaultDelete,
            ("WSACONTEXT", Some("BUILD")) => Self::WsaContextBuild,
            ("WSACONTEXT", Some("DELETE")) => Self::WsaContextDelete,
            ("WSACONTEXT", Some("GET")) => Self::WsaContextGet,
            ("WSAEPR", Some("CREATE")) => Self::WsaEprCreate,
            ("LOAD", _) => Self::Load,
            ("RELEASE", _) => Self::Release,
            ("LINK", _) => Self::Link,
            ("POP", Some("HANDLE")) => Self::PopHandle,
            ("PUSH", Some("HANDLE")) => Self::PushHandle,
            ("PURGE", Some("MESSAGE")) => Self::PurgeMessage,
            ("READ", _) => Self::Read,
            ("READQ", Some("TD")) => Self::ReadTransientData,
            ("REMOVE", Some("SUBEVENT")) => Self::RemoveSubevent,
            ("READNEXT", _) => Self::ReadNext,
            ("READPREV", _) => Self::ReadPrev,
            ("RECEIVE", Some("MAP")) => Self::ReceiveMap,
            ("RECEIVE", Some("PARTN")) => Self::ReceivePartn,
            ("RETRIEVE", Some("REATTACH")) => Self::RetrieveReattachEvent,
            ("RETRIEVE", Some("SUBEVENT")) => Self::RetrieveSubevent,
            ("RETRIEVE", _) => Self::Retrieve,
            ("TEST", Some("EVENT")) => Self::TestEvent,
            ("SIGNAL", Some("EVENT")) => Self::SignalEvent,
            ("RETURN", _) => Self::Return,
            ("RESETBR", _) => Self::ResetBrowse,
            ("REWRITE", _) => Self::Rewrite,
            ("SEND", Some("MAP")) => Self::SendMap,
            ("SEND", Some("CONTROL")) => Self::SendControl,
            ("ISSUE", Some("ABORT")) => Self::IssueAbort,
            ("ISSUE", Some("ADD")) => Self::IssueAdd,
            ("ISSUE", Some("END")) => Self::IssueEnd,
            ("ISSUE", Some("ERASE")) => Self::IssueErase,
            ("ISSUE", Some("NOTE")) => Self::IssueNote,
            ("ISSUE", Some("QUERY")) => Self::IssueQuery,
            ("ISSUE", Some("RECEIVE")) => Self::IssueReceive,
            ("ISSUE", Some("REPLACE")) => Self::IssueReplace,
            ("ISSUE", Some("SEND")) => Self::IssueSend,
            ("ROUTE", _) => Self::Route,
            ("ISSUE", Some("WAIT")) => Self::IssueWait,
            ("SEND", Some("PAGE")) => Self::SendPage,
            ("SEND", Some("PARTNSET")) => Self::SendPartnset,
            ("SEND", _) => Self::SendText,
            ("SET", Some("ASSOCIATION")) => Self::SetAssociationUserCorrData,
            ("SPOOLCLOSE", _) => Self::SpoolClose,
            ("SPOOLOPEN", Some("INPUT")) => Self::SpoolOpenInput,
            ("SPOOLOPEN", Some("OUTPUT")) => Self::SpoolOpenOutput,
            ("SPOOLREAD", _) => Self::SpoolRead,
            ("SPOOLWRITE", _) => Self::SpoolWrite,
            ("ENTER", Some("TRACENUM")) => Self::EnterTraceNum,
            ("MONITOR", _) => Self::Monitor,
            ("DUMP", Some("TRANSACTION")) => Self::DumpTransaction,
            ("DUMP", _) => Self::Dump,
            ("TRACE", _) => Self::Trace,
            ("ENTER", Some("TRACEID")) => Self::EnterTraceId,
            ("START", _) => Self::Start,
            ("STARTBR", _) => Self::StartBrowse,
            ("SUSPEND", _) => Self::Suspend,
            ("WAIT", Some("EVENT")) => Self::WaitEvent,
            ("WAIT", Some("EXTERNAL")) => Self::WaitExternal,
            ("SYNCPOINT", _) => Self::Syncpoint,
            ("TRANSFORM", Some("DATATOJSON")) => Self::TransformDataToJson,
            ("TRANSFORM", Some("DATATOXML")) => Self::TransformDataToXml,
            ("TRANSFORM", Some("JSONTODATA")) => Self::TransformJsonToData,
            ("TRANSFORM", Some("XMLTODATA")) => Self::TransformXmlToData,
            ("WEB", Some("PARSE")) => Self::WebParseUrl,
            ("WAIT", Some("JOURNALNAME")) => Self::WaitJournalName,
            ("WAIT", Some("JOURNALNUM")) => Self::WaitJournalNum,
            ("WRITE", Some("JOURNALNAME")) => Self::WriteJournalName,
            ("WRITE", Some("JOURNALNUM")) => Self::WriteJournalNum,
            ("UNLOCK", _) => Self::Unlock,
            ("WRITE", _) => Self::Write,
            ("WRITEQ", Some("TD")) => Self::WriteTransientData,
            ("XCTL", _) => Self::Xctl,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn supported(self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CicsConditionPolicy {
    Default,
    NoHandle,
    Respond {
        response_field: String,
        response2_field: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsRequest {
    pub operation: CicsOperation,
    pub arguments: BTreeMap<String, BoundedPayload>,
    pub condition_policy: CicsConditionPolicy,
    pub mutation: Option<Mutation>,
}

impl CicsRequest {
    pub(super) fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.arguments.len() > limits.max_fields {
            return Err(HostProblem::ResourceExhausted);
        }
        if self.is_mutating() {
            self.mutation
                .as_ref()
                .ok_or(HostProblem::MissingIdempotency)?
                .validate(limits)?;
        }
        Ok(())
    }

    /// A token-producing update read changes task state and needs outer replay.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        self.operation.is_mutating()
            || matches!(
                self.operation,
                CicsOperation::Read | CicsOperation::ReadNext | CicsOperation::ReadPrev
            ) && self.arguments.contains_key("TOKEN")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsDisposition {
    Complete,
    /// A default-handled condition completed without transferring control.
    Ignored,
    Suspended,
    Transfer,
    Handler,
    Returned,
    Abended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsUnitOfWorkOutcome {
    Committed,
    RolledBack,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsResponse {
    pub disposition: CicsDisposition,
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub applid: String,
    pub sysid: String,
    pub transaction: String,
    pub aid: u8,
    pub target: Option<String>,
    pub next_transaction: Option<String>,
    pub payload: BoundedPayload,
    pub outputs: BTreeMap<String, BoundedPayload>,
    pub unit_of_work: Option<CicsUnitOfWorkOutcome>,
}
