//! Typed CICS host request and result boundary.

use super::{HostLimits, HostProblem, Mutation};
use mainframe_env_execution_api::BoundedPayload;
use std::collections::BTreeMap;

/// Typed CICS operations admitted at the host request boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsOperation {
    Abend,
    /// Return checked virtual addresses for task storage areas.
    Address,
    /// Copy one checked virtual pointer/address relationship.
    AddressSet,
    Asktime,
    /// Remove editing characters from one numeric field in place.
    BifDeedit,
    /// Calculate a bounded SHA-1 digest of caller supplied data.
    BifDigest,
    /// Refresh only the implicit EIB date and time fields.
    AsktimeEib,
    Assign,
    /// Cancel one unhonored local interval-control START request.
    Cancel,
    /// Change the issuing CICS task's dispatch priority.
    ChangeTask,
    /// Complete the source-defined zero-delay interval-control boundary.
    Delay,
    /// Arm one task-owned timer-event control area.
    Post,
    /// Write one system-console message and optionally await its reply.
    WriteOperator,
    /// Release one matching task enqueue ownership level.
    Deq,
    /// Delete the current file record.
    Delete,
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
    /// Convert a 64-byte architected date-time string to packed absolute time.
    ConvertTime,
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
    ReceiveMap,
    Retrieve,
    Return,
    Rewrite,
    SendText,
    SendMap,
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
    /// Schedule one interval-control START record.
    Start,
    StartBrowse,
    /// Relinquish control until the task is redispatched.
    Suspend,
    /// Wait for one timer-event control area to be posted.
    WaitEvent,
    /// Wait for standard MVS posting of one ECB in a bounded external list.
    WaitExternal,
    /// Wait on one or more MVS-format ECBs, including hand-posted events.
    WaitCics,
    Syncpoint,
    /// Convert one BIT-mode application-data container to canonical JSON.
    TransformDataToJson,
    /// Convert one BIT-mode application-data container to deterministic XML.
    TransformDataToXml,
    /// Convert JSON from a channel container to application data.
    TransformJsonToData,
    /// Query XML metadata or convert an XML container to application data.
    TransformXmlToData,
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
            Self::Address => "Address",
            Self::AddressSet => "AddressSet",
            Self::Asktime => "Asktime",
            Self::BifDeedit => "BifDeedit",
            Self::BifDigest => "BifDigest",
            Self::AsktimeEib => "AsktimeEib",
            Self::Assign => "Assign",
            Self::Cancel => "Cancel",
            Self::ChangeTask => "ChangeTask",
            Self::Delay => "Delay",
            Self::Post => "Post",
            Self::WriteOperator => "WriteOperator",
            Self::Deq => "Deq",
            Self::Delete => "Delete",
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
            Self::ConvertTime => "ConvertTime",
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
            Self::ReceiveMap => "ReceiveMap",
            Self::Retrieve => "Retrieve",
            Self::Return => "Return",
            Self::Rewrite => "Rewrite",
            Self::SendText => "SendText",
            Self::SendMap => "SendMap",
            Self::SetAssociationUserCorrData => "SetAssociationUserCorrData",
            Self::SetFileStatus => "SetFileStatus",
            Self::SpoolClose => "SpoolClose",
            Self::SpoolOpenInput => "SpoolOpenInput",
            Self::SpoolOpenOutput => "SpoolOpenOutput",
            Self::SpoolRead => "SpoolRead",
            Self::SpoolWrite => "SpoolWrite",
            Self::Start => "Start",
            Self::StartBrowse => "StartBrowse",
            Self::Suspend => "Suspend",
            Self::WaitEvent => "WaitEvent",
            Self::WaitExternal => "WaitExternal",
            Self::WaitCics => "WaitCics",
            Self::Syncpoint => "Syncpoint",
            Self::TransformDataToJson => "TransformDataToJson",
            Self::TransformDataToXml => "TransformDataToXml",
            Self::TransformJsonToData => "TransformJsonToData",
            Self::TransformXmlToData => "TransformXmlToData",
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

    /// Whether the operation may change durable or task-local state.
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::Delete
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
                | Self::Post
                | Self::WriteOperator
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
                | Self::Load
                | Self::Release
                | Self::ReceiveMap
                | Self::ReadTransientData
                | Self::PurgeMessage
                | Self::SendMap
                | Self::SendText
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
                | Self::Start
                | Self::Retrieve
                | Self::WaitEvent
                | Self::WaitExternal
                | Self::WaitCics
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
            ("BIF", Some("DEEDIT")) => Self::BifDeedit,
            ("BIF", Some("DIGEST")) => Self::BifDigest,
            ("ASSIGN", _) => Self::Assign,
            ("CANCEL", _) => Self::Cancel,
            ("CHANGE", Some("TASK")) => Self::ChangeTask,
            ("DELAY", _) => Self::Delay,
            ("POST", _) => Self::Post,
            ("WRITE", Some("OPERATOR")) => Self::WriteOperator,
            ("DEQ", _) => Self::Deq,
            ("DELETE", _) => Self::Delete,
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
            ("CONVERTTIME", _) => Self::ConvertTime,
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
            ("LOAD", _) => Self::Load,
            ("RELEASE", _) => Self::Release,
            ("LINK", _) => Self::Link,
            ("POP", Some("HANDLE")) => Self::PopHandle,
            ("PUSH", Some("HANDLE")) => Self::PushHandle,
            ("PURGE", Some("MESSAGE")) => Self::PurgeMessage,
            ("READ", _) => Self::Read,
            ("READQ", Some("TD")) => Self::ReadTransientData,
            ("READNEXT", _) => Self::ReadNext,
            ("READPREV", _) => Self::ReadPrev,
            ("RECEIVE", Some("MAP")) => Self::ReceiveMap,
            ("RETRIEVE", _) => Self::Retrieve,
            ("RETURN", _) => Self::Return,
            ("RESETBR", _) => Self::ResetBrowse,
            ("REWRITE", _) => Self::Rewrite,
            ("SEND", Some("MAP")) => Self::SendMap,
            ("SEND", _) => Self::SendText,
            ("SET", Some("ASSOCIATION")) => Self::SetAssociationUserCorrData,
            ("SPOOLCLOSE", _) => Self::SpoolClose,
            ("SPOOLOPEN", Some("INPUT")) => Self::SpoolOpenInput,
            ("SPOOLOPEN", Some("OUTPUT")) => Self::SpoolOpenOutput,
            ("SPOOLREAD", _) => Self::SpoolRead,
            ("SPOOLWRITE", _) => Self::SpoolWrite,
            ("START", _) => Self::Start,
            ("STARTBR", _) => Self::StartBrowse,
            ("SUSPEND", _) => Self::Suspend,
            ("WAIT", Some("EVENT")) => Self::WaitEvent,
            ("WAIT", Some("EXTERNAL")) => Self::WaitExternal,
            ("WAITCICS", _) => Self::WaitCics,
            ("SYNCPOINT", _) => Self::Syncpoint,
            ("TRANSFORM", Some("DATATOJSON")) => Self::TransformDataToJson,
            ("TRANSFORM", Some("DATATOXML")) => Self::TransformDataToXml,
            ("TRANSFORM", Some("JSONTODATA")) => Self::TransformJsonToData,
            ("TRANSFORM", Some("XMLTODATA")) => Self::TransformXmlToData,
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
