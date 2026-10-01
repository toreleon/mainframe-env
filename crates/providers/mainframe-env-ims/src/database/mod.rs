//! Metadata-driven, deterministic IMS database algorithms.
//!
//! This module is an isolated engine foundation. It does not register a host
//! provider or map internal conditions to PCB status codes. Those integration
//! steps remain outside this engine foundation.

use self::definition::validate_definition;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use crate::metadata::ImsDatabaseOrganization as DatabaseOrganization;

/// Explicit resource bounds for one database engine instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineLimits {
    pub max_segments: usize,
    pub max_fields_per_segment: usize,
    pub max_secondary_indexes: usize,
    pub max_records: usize,
    pub max_children_per_parent: usize,
    pub max_segment_bytes: usize,
    pub max_name_bytes: usize,
    pub max_predicates: usize,
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            max_segments: 64,
            max_fields_per_segment: 64,
            max_secondary_indexes: 64,
            max_records: 65_536,
            max_children_per_parent: 4_096,
            max_segment_bytes: 32 * 1024,
            max_name_bytes: 64,
            max_predicates: 64,
        }
    }
}

/// One named field within a segment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldDefinition {
    pub name: String,
    pub offset: usize,
    pub length: usize,
}

/// One segment type in the database hierarchy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SegmentDefinition {
    pub name: String,
    pub parent: Option<String>,
    pub min_length: usize,
    pub max_length: usize,
    pub key_field: Option<String>,
    pub fields: Vec<FieldDefinition>,
}

/// A secondary access path maintained from one source-segment field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecondaryIndexDefinition {
    pub name: String,
    pub source_segment: String,
    pub field: String,
    pub unique: bool,
}

/// Immutable metadata consumed by the algorithm foundation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DatabaseDefinition {
    pub name: String,
    pub organization: DatabaseOrganization,
    pub segments: Vec<SegmentDefinition>,
    pub secondary_indexes: Vec<SecondaryIndexDefinition>,
}

/// Stable record identity within one engine image.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RecordId(u64);

/// Read-only record returned by navigation and index lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordView {
    pub id: RecordId,
    pub segment: String,
    pub parent: Option<RecordId>,
    pub data: Vec<u8>,
}

/// A bounded insert request. The parent is absent only for root/GSAM records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsertRequest {
    pub segment: String,
    pub parent: Option<RecordId>,
    pub data: Vec<u8>,
}

/// Supported field comparison forms for qualified path selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Relation {
    Equal,
    NotEqual,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

/// One qualified field predicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldPredicate {
    pub field: String,
    pub relation: Relation,
    pub value: Vec<u8>,
}

/// Predicates for one segment type in a hierarchical path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentSelector {
    pub segment: String,
    pub predicates: Vec<FieldPredicate>,
}

/// Navigation family implemented by the isolated engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadKind {
    Unique,
    Next,
    NextInParent,
}

/// One metadata-resolved navigation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadRequest {
    pub kind: ReadKind,
    pub target: Option<String>,
    pub path: Vec<SegmentSelector>,
    pub hold: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct HeldRecord {
    id: RecordId,
    version: u64,
}

/// Position and parentage owned by one future DB PCB adapter.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PcbPosition {
    current: Option<RecordId>,
    parentage: Option<RecordId>,
    held: Option<HeldRecord>,
    after_end: bool,
}

impl PcbPosition {
    pub fn current(&self) -> Option<RecordId> {
        self.current
    }

    pub fn parentage(&self) -> Option<RecordId> {
        self.parentage
    }

    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }
}

/// Fail-closed internal condition. PCB status mapping is intentionally absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineProblem {
    InvalidDefinition,
    InvalidRequest,
    InvalidData,
    LimitExceeded,
    Unsupported,
    NotFound,
    EndOfDatabase,
    Duplicate,
    PathMismatch,
    ParentageRequired,
    HoldRequired,
    StaleHold,
    KeyChange,
    IndexConflict,
}

impl fmt::Display for EngineProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDefinition => "invalid database definition",
            Self::InvalidRequest => "invalid database request",
            Self::InvalidData => "invalid segment data",
            Self::LimitExceeded => "database engine limit exceeded",
            Self::Unsupported => "operation unsupported by database organization",
            Self::NotFound => "segment not found",
            Self::EndOfDatabase => "end of database",
            Self::Duplicate => "duplicate segment key",
            Self::PathMismatch => "qualified path does not match retained position",
            Self::ParentageRequired => "parentage is not established",
            Self::HoldRequired => "a current held segment is required",
            Self::StaleHold => "held segment is stale",
            Self::KeyChange => "segment key cannot be replaced",
            Self::IndexConflict => "secondary index uniqueness conflict",
        })
    }
}

impl std::error::Error for EngineProblem {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Record {
    id: RecordId,
    segment: String,
    parent: Option<RecordId>,
    data: Vec<u8>,
    children: Vec<RecordId>,
    version: u64,
}

type IndexValues = Vec<(String, Option<Vec<u8>>)>;

/// Cloneable deterministic database image. Persistence remains owned by the
/// shared `ProviderStateStore` integration layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseEngine {
    definition: DatabaseDefinition,
    limits: EngineLimits,
    records: BTreeMap<RecordId, Record>,
    roots: Vec<RecordId>,
    indexes: BTreeMap<String, BTreeMap<Vec<u8>, BTreeSet<RecordId>>>,
    next_id: u64,
    revision: u64,
}

mod definition;
mod navigation;
mod store;

impl DatabaseEngine {
    /// Validate metadata before accepting any state.
    pub fn new(
        definition: DatabaseDefinition,
        limits: EngineLimits,
    ) -> Result<Self, EngineProblem> {
        validate_definition(&definition, limits)?;
        let indexes = definition
            .secondary_indexes
            .iter()
            .map(|index| (index.name.clone(), BTreeMap::new()))
            .collect();
        Ok(Self {
            definition,
            limits,
            records: BTreeMap::new(),
            roots: Vec::new(),
            indexes,
            next_id: 1,
            revision: 0,
        })
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// Read-only image projection used by bounded utility unload/reload.
    pub fn definition(&self) -> &DatabaseDefinition {
        &self.definition
    }

    /// Stable insertion-order views; parents precede children by record ID.
    pub fn export_records(&self) -> Vec<RecordView> {
        self.records
            .keys()
            .copied()
            .map(|id| self.view(id))
            .collect()
    }

    /// A deterministic state identity used for failure/retry assertions.
    pub fn state_digest(&self) -> [u8; 32] {
        let indexes = self
            .indexes
            .iter()
            .map(|(name, entries)| (name, entries.iter().collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        let material = serde_json::to_vec(&(
            "mainframe-env.ims-database-engine@1",
            &self.definition,
            self.next_id,
            self.revision,
            self.records.values().collect::<Vec<_>>(),
            &self.roots,
            indexes,
        ))
        .expect("bounded database state has an infallible JSON representation");
        Sha256::digest(material).into()
    }
}

#[cfg(test)]
mod tests;
