//! Typed CICS host request and result boundary.

use super::Mutation;
use mainframe_env_execution_api::BoundedPayload;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsOperation {
    Abend,
    /// Copy one checked virtual pointer/address relationship.
    AddressSet,
    Asktime,
    Assign,
    /// Change the issuing CICS task's dispatch priority.
    ChangeTask,
    /// Release one matching task enqueue ownership level.
    Deq,
    Delete,
    /// Acquire or wait for one task enqueue resource.
    Enq,
    EndBrowse,
    FormatTime,
    HandleAbend,
    HandleCondition,
    Inquire,
    Link,
    /// Restore one suspended HANDLE/IGNORE specification snapshot.
    PopHandle,
    /// Suspend the current HANDLE/IGNORE specifications in one nested snapshot.
    PushHandle,
    Read,
    ReadNext,
    ReadPrev,
    ReceiveMap,
    Retrieve,
    Return,
    Rewrite,
    SendText,
    SendMap,
    /// Overwrite the originating task's bounded user correlator data.
    SetAssociationUserCorrData,
    SetFileStatus,
    StartBrowse,
    /// Relinquish control until the task is redispatched.
    Suspend,
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
            Self::AddressSet => "AddressSet",
            Self::Asktime => "Asktime",
            Self::Assign => "Assign",
            Self::ChangeTask => "ChangeTask",
            Self::Deq => "Deq",
            Self::Delete => "Delete",
            Self::Enq => "Enq",
            Self::EndBrowse => "EndBrowse",
            Self::FormatTime => "FormatTime",
            Self::HandleAbend => "HandleAbend",
            Self::HandleCondition => "HandleCondition",
            Self::Inquire => "Inquire",
            Self::Link => "Link",
            Self::PopHandle => "PopHandle",
            Self::PushHandle => "PushHandle",
            Self::Read => "Read",
            Self::ReadNext => "ReadNext",
            Self::ReadPrev => "ReadPrev",
            Self::ReceiveMap => "ReceiveMap",
            Self::Retrieve => "Retrieve",
            Self::Return => "Return",
            Self::Rewrite => "Rewrite",
            Self::SendText => "SendText",
            Self::SendMap => "SendMap",
            Self::SetAssociationUserCorrData => "SetAssociationUserCorrData",
            Self::SetFileStatus => "SetFileStatus",
            Self::StartBrowse => "StartBrowse",
            Self::Suspend => "Suspend",
            Self::Syncpoint => "Syncpoint",
            Self::Write => "Write",
            Self::WriteTransientData => "WriteTransientData",
            Self::Xctl => "Xctl",
        }
    }

    #[must_use]
    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::Delete
                | Self::Deq
                | Self::Enq
                | Self::Rewrite
                | Self::Write
                | Self::WriteTransientData
                | Self::Link
                | Self::ReceiveMap
                | Self::SendMap
                | Self::SendText
                | Self::SetAssociationUserCorrData
                | Self::Xctl
                | Self::Return
                | Self::Abend
                | Self::Syncpoint
                | Self::SetFileStatus
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
            ("ASKTIME", _) => Self::Asktime,
            ("ASSIGN", _) => Self::Assign,
            ("CHANGE", Some("TASK")) => Self::ChangeTask,
            ("DEQ", _) => Self::Deq,
            ("DELETE", _) => Self::Delete,
            ("ENQ", _) => Self::Enq,
            ("ENDBR", _) => Self::EndBrowse,
            ("FORMATTIME", _) => Self::FormatTime,
            ("HANDLE", Some("ABEND")) => Self::HandleAbend,
            ("HANDLE", _) => Self::HandleCondition,
            ("INQUIRE", _) => Self::Inquire,
            ("LINK", _) => Self::Link,
            ("POP", Some("HANDLE")) => Self::PopHandle,
            ("PUSH", Some("HANDLE")) => Self::PushHandle,
            ("READ", _) => Self::Read,
            ("READNEXT", _) => Self::ReadNext,
            ("READPREV", _) => Self::ReadPrev,
            ("RECEIVE", Some("MAP")) => Self::ReceiveMap,
            ("RETRIEVE", _) => Self::Retrieve,
            ("RETURN", _) => Self::Return,
            ("REWRITE", _) => Self::Rewrite,
            ("SEND", Some("MAP")) => Self::SendMap,
            ("SEND", _) => Self::SendText,
            ("SET", Some("ASSOCIATION")) => Self::SetAssociationUserCorrData,
            ("STARTBR", _) => Self::StartBrowse,
            ("SUSPEND", _) => Self::Suspend,
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
