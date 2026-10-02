use super::Mutation;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2Operation {
    ExecuteScript,
    FreePlans,
    Select,
    Insert,
    Update,
    Delete,
    Count,
    DeclareCursor,
    OpenCursor,
    FetchCursor,
    CloseCursor,
    Commit,
    Rollback,
    Extract,
}

impl Db2Operation {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Select | Self::Count | Self::Extract)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2HostVariable {
    pub value: Vec<u8>,
    pub indicator: Option<i16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Request {
    pub operation: Db2Operation,
    pub statement: String,
    pub cursor: Option<String>,
    pub inputs: BTreeMap<String, Db2HostVariable>,
    pub outputs: Vec<String>,
    pub max_rows: u32,
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Row {
    pub columns: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Result {
    pub sqlcode: i32,
    pub sqlstate: String,
    pub message: String,
    pub rows: Vec<Db2Row>,
    pub affected_rows: u64,
}
