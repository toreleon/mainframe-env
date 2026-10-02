use super::Mutation;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Typed relational statement, cursor or transaction operation.
/// The selector's replay classification includes cursor and plan state changes.
pub enum Db2Operation {
    /// Execute the supplied statement script.
    ExecuteScript,
    /// Release retained execution plans.
    FreePlans,
    /// Select rows without mutation replay classification.
    Select,
    /// Insert rows selected by the statement.
    Insert,
    /// Update rows selected by the statement.
    Update,
    /// Delete rows selected by the statement.
    Delete,
    /// Return a count from the selected query.
    Count,
    /// Declare a named cursor for the statement.
    DeclareCursor,
    /// Open the selected declared cursor.
    OpenCursor,
    /// Fetch a bounded page from the cursor.
    FetchCursor,
    /// Close the selected cursor.
    CloseCursor,
    /// Commit the relational unit of work.
    Commit,
    /// Roll back the relational unit of work.
    Rollback,
    /// Extract provider metadata without mutation replay classification.
    Extract,
}

impl Db2Operation {
    #[must_use]
    /// Whether the selector requires mutation replay metadata.
    /// Only `Select`, `Count` and `Extract` are classified as read-only.
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Select | Self::Count | Self::Extract)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned host-variable bytes with an optional signed indicator.
pub struct Db2HostVariable {
    /// Owned value bytes bounded by `max_record_bytes`; no implicit text decoding is performed.
    pub value: Vec<u8>,
    /// Optional signed indicator interpreted by the statement/provider contract.
    pub indicator: Option<i16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned relational call operands with bounded statement, variables and row count.
/// Mutating selectors require replay metadata; SQL applicability belongs to the provider.
pub struct Db2Request {
    /// Typed statement, cursor or transaction selector.
    pub operation: Db2Operation,
    /// Owned statement text with byte length at most `max_state_bytes`.
    pub statement: String,
    /// Optional nonempty cursor identifier bounded by `max_name_bytes`.
    pub cursor: Option<String>,
    /// Named input variables; count is bounded by `max_fields` and names by `max_name_bytes`.
    pub inputs: BTreeMap<String, Db2HostVariable>,
    /// Ordered output variable names, bounded by `max_fields` and `max_name_bytes`.
    pub outputs: Vec<String>,
    /// Requested row ceiling, no greater than `max_records`.
    pub max_rows: u32,
    /// Replay metadata required for selectors classified as mutating.
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One row of owned column bytes in provider-returned order.
pub struct Db2Row {
    /// Owned column byte values; count and each value are host-bounded.
    pub columns: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Relational reply with SQL diagnostics, bounded rows and affected-row count.
pub struct Db2Result {
    /// Signed SQL completion code returned by the provider.
    pub sqlcode: i32,
    /// Exactly five bytes of SQL state text, as checked by host reply validation.
    pub sqlstate: String,
    /// Provider diagnostic text bounded by `max_state_bytes`.
    pub message: String,
    /// Returned rows bounded by `max_records`.
    pub rows: Vec<Db2Row>,
    /// Provider-reported count of rows affected by the operation.
    pub affected_rows: u64,
}
