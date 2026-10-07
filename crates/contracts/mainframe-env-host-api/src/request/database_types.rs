//! Db2 request/result records, without engine dispatch or memory pointers.

use super::Mutation;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Modeled Db2 operation selector; cursor and transaction operations count as mutations at this boundary.
pub enum Db2Operation {
    /// Execute a script, classified as mutating.
    ExecuteScript,
    /// Release plan state, classified as mutating.
    FreePlans,
    /// Read rows without requesting mutation identity.
    Select,
    /// Insert rows under mutation identity.
    Insert,
    /// Update rows under mutation identity.
    Update,
    /// Delete rows under mutation identity.
    Delete,
    /// Read a count without requesting mutation identity.
    Count,
    /// Declare retained cursor state.
    DeclareCursor,
    /// Open retained cursor state.
    OpenCursor,
    /// Advance retained cursor state.
    FetchCursor,
    /// Release retained cursor state.
    CloseCursor,
    /// Request transaction commit; the selector alone is not a commit receipt.
    Commit,
    /// Request transaction rollback.
    Rollback,
    /// Extract observations without requesting mutation identity.
    Extract,
}

impl Db2Operation {
    #[must_use]
    /// Classify all but Select, Count and Extract as replay-requiring mutations, including cursor state changes.
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Select | Self::Count | Self::Extract)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned host-variable bytes plus an optional signed indicator, without a wire pointer or inferred SQL type.
pub struct Db2HostVariable {
    /// Exact owned host-variable bytes, bounded by max_record_bytes.
    pub value: Vec<u8>,
    /// Optional signed SQL indicator retained without inferring value type or nullability here.
    pub indicator: Option<i16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Bounded SQL request data. Shape admission does not establish SQL legality or database permission.
pub struct Db2Request {
    /// Explicit SQL/cursor/transaction operation selector.
    pub operation: Db2Operation,
    /// Statement or script text bounded by max_state_bytes; validation is not a SQL parser.
    pub statement: String,
    /// Optional nonempty bounded cursor name.
    pub cursor: Option<String>,
    /// Bounded named input variables; names and values are checked separately.
    pub inputs: BTreeMap<String, Db2HostVariable>,
    /// Ordered bounded output variable names.
    pub outputs: Vec<String>,
    /// Requested row ceiling, no greater than max_records; zero is structurally permitted.
    pub max_rows: u32,
    /// Original replay identity required before mutation dispatch; it does not confer permission.
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One result row preserving column byte order; decoding belongs to the SQL/application contract.
pub struct Db2Row {
    /// Column bytes in result order, with field-count and individual-byte bounds.
    pub columns: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// SQL completion fields and owned rows retained without translating SQL failures into infrastructure success.
pub struct Db2Result {
    /// Signed SQL completion code retained exactly.
    pub sqlcode: i32,
    /// Five-byte SQLSTATE spelling; structural validation checks length only.
    pub sqlstate: String,
    /// Bounded provider diagnostic text.
    pub message: String,
    /// Bounded rows with ordered column bytes.
    pub rows: Vec<Db2Row>,
    /// Provider-reported affected row count, independent of returned rows.
    pub affected_rows: u64,
}
