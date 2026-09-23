//! Typed CICS host request and result boundary.

use super::Mutation;
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
    /// Refresh only the implicit EIB date and time fields.
    AsktimeEib,
    Assign,
    /// Cancel one unhonored local interval-control START request.
    Cancel,
    /// Change the issuing CICS task's dispatch priority.
    ChangeTask,
    /// Complete the source-defined zero-delay interval-control boundary.
    Delay,
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
    /// Release one task-local virtual storage area acquired by GETMAIN.
    Freemain,
    /// Allocate one bounded task-local virtual storage area.
    Getmain,
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
            Self::AsktimeEib => "AsktimeEib",
            Self::Assign => "Assign",
            Self::Cancel => "Cancel",
            Self::ChangeTask => "ChangeTask",
            Self::Delay => "Delay",
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
            Self::Freemain => "Freemain",
            Self::Getmain => "Getmain",
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
            Self::ReadTransientData => "ReadTransientData",
            Self::ReceiveMap => "ReceiveMap",
            Self::Retrieve => "Retrieve",
            Self::Return => "Return",
            Self::Rewrite => "Rewrite",
            Self::SendText => "SendText",
            Self::SendMap => "SendMap",
            Self::SetAssociationUserCorrData => "SetAssociationUserCorrData",
            Self::SetFileStatus => "SetFileStatus",
            Self::Start => "Start",
            Self::StartBrowse => "StartBrowse",
            Self::Suspend => "Suspend",
            Self::WaitEvent => "WaitEvent",
            Self::WaitExternal => "WaitExternal",
            Self::Syncpoint => "Syncpoint",
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
                | Self::DeleteTransientData
                | Self::DeleteTemporaryStorage
                | Self::ReadTemporaryStorage
                | Self::WriteTemporaryStorage
                | Self::Cancel
                | Self::Delay
                | Self::Deq
                | Self::Enq
                | Self::Freemain
                | Self::Getmain
                | Self::Rewrite
                | Self::Write
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
                | Self::SetFileStatus
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
            ("FREEMAIN", _) => Self::Freemain,
            ("GETMAIN", _) => Self::Getmain,
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
            ("REWRITE", _) => Self::Rewrite,
            ("SEND", Some("MAP")) => Self::SendMap,
            ("SEND", _) => Self::SendText,
            ("SET", Some("ASSOCIATION")) => Self::SetAssociationUserCorrData,
            ("START", _) => Self::Start,
            ("STARTBR", _) => Self::StartBrowse,
            ("SUSPEND", _) => Self::Suspend,
            ("WAIT", Some("EVENT")) => Self::WaitEvent,
            ("WAIT", Some("EXTERNAL")) => Self::WaitExternal,
            ("SYNCPOINT", _) => Self::Syncpoint,
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
