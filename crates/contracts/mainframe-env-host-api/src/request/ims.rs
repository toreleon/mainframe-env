//! Existing IMS database request/result records behind stable re-exports.
use super::Mutation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Operation identity for the existing metadata-selected local database route.
pub enum ImsOperation {
    /// Bind the run to an installed PSB through the existing schedule route.
    Schedule,
    /// End the run through the existing termination route.
    Terminate,
    /// Select through the unique-navigation route.
    GetUnique,
    /// Advance through the next-navigation route.
    GetNext,
    /// Advance within the retained parent-navigation scope.
    GetNextParent,
    /// Unique navigation requesting the existing hold authority.
    GetHoldUnique,
    /// Next navigation requesting the existing hold authority.
    GetHoldNext,
    /// Parent-scoped navigation requesting the existing hold authority.
    GetHoldNextParent,
    /// Insert through the existing bounded database mutation route.
    Insert,
    /// Replace through the existing hold-gated mutation route.
    Replace,
    /// Delete through the existing hold-gated mutation route.
    Delete,
    /// Existing generic checkpoint operation, distinct from typed application recovery operands.
    Checkpoint,
    /// Load an owned bounded image through the existing administrative route.
    Load,
    /// Return the existing bounded image projection.
    Unload,
    /// Settle the existing local unit of work by commit.
    Commit,
    /// Back out the existing fenced local unit of work.
    Rollback,
    /// Dispatch the separately typed system-call operands.
    System,
}

impl ImsOperation {
    #[must_use]
    /// Classify every operation except Unload as stateful and requiring replay identity.
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Unload)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned segment/field equality qualifier with exact comparative bytes.
pub struct ImsQualifier {
    /// Segment identity whose field is qualified.
    pub segment: String,
    /// Metadata field identity within the segment.
    pub field: String,
    /// Exact comparative bytes, bounded by host validation.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Existing bounded database-call DTO; selected metadata and provider fences remain authoritative.
pub struct ImsRequest {
    /// Typed operation selecting the existing request family.
    pub operation: ImsOperation,
    /// PSB identity for scheduling; absent for already-bound operation forms.
    pub psb: Option<String>,
    /// Selected positive PCB number where required by the operation.
    pub pcb: u16,
    /// Ordered segment identities for legacy operands; empty on the rich SSA route.
    pub segments: Vec<String>,
    /// Caller segment or image bytes for applicable mutation forms.
    pub data: Vec<u8>,
    /// Legacy field qualifiers; excluded when raw SSA operands are supplied.
    pub qualifiers: Vec<ImsQualifier>,
    /// Optional logical checkpoint identity for the generic checkpoint form.
    pub checkpoint_id: Option<String>,
    /// Positive output segment-count bound checked against host limits.
    pub max_segments: u32,
    /// Replay identity required for stateful operations, including positioning Gets.
    pub mutation: Option<Mutation>,
    /// Typed system-call operands. Present only for `ImsOperation::System`.
    pub system: Option<crate::ImsSystemRequest>,
    /// Q/LOCKCLASS reservation requested by a database Get call.
    pub q_class: Option<crate::ImsQClass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned returned segment bytes and optional parent identity from the existing route.
pub struct ImsSegment {
    /// Returned segment resource name.
    pub name: String,
    /// Optional exact parent-key bytes; absent when no parent identity is returned.
    pub parent_key: Option<Vec<u8>>,
    /// Returned segment bytes, which may be suppressed by an admitted key-only route.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Bounded owned database-call result; status and transferred data remain distinct observations.
pub struct ImsResult {
    /// Exact two-byte status text checked by the host result boundary.
    pub status: String,
    /// Returned segment observations bounded by host limits.
    pub segments: Vec<ImsSegment>,
    /// Checkpoint identity when the operation returns one; otherwise absent.
    pub checkpoint_id: Option<String>,
    /// Count of affected segments reported by the operation.
    pub affected_segments: u64,
    /// Typed output for a system call; absent for existing database/TM calls.
    pub system: Option<crate::ImsSystemResult>,
}
