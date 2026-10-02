//! Existing IMS database request/result records behind stable re-exports.
use super::Mutation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsOperation {
    Schedule,
    Terminate,
    GetUnique,
    GetNext,
    GetNextParent,
    GetHoldUnique,
    GetHoldNext,
    GetHoldNextParent,
    Insert,
    Replace,
    Delete,
    Checkpoint,
    Load,
    Unload,
    Commit,
    Rollback,
    System,
}

impl ImsOperation {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Unload)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsQualifier {
    pub segment: String,
    pub field: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsRequest {
    pub operation: ImsOperation,
    pub psb: Option<String>,
    pub pcb: u16,
    pub segments: Vec<String>,
    pub data: Vec<u8>,
    pub qualifiers: Vec<ImsQualifier>,
    pub checkpoint_id: Option<String>,
    pub max_segments: u32,
    pub mutation: Option<Mutation>,
    /// Typed system-call operands. Present only for `ImsOperation::System`.
    pub system: Option<crate::ImsSystemRequest>,
    /// Q/LOCKCLASS reservation requested by a database Get call.
    pub q_class: Option<crate::ImsQClass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsSegment {
    pub name: String,
    pub parent_key: Option<Vec<u8>>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsResult {
    pub status: String,
    pub segments: Vec<ImsSegment>,
    pub checkpoint_id: Option<String>,
    pub affected_segments: u64,
    /// Typed output for a system call; absent for existing database/TM calls.
    pub system: Option<crate::ImsSystemResult>,
}
