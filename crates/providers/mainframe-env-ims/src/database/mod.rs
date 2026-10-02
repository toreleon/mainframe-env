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

pub(crate) fn is_index_database(organization: DatabaseOrganization) -> bool {
    matches!(
        organization,
        DatabaseOrganization::Index | DatabaseOrganization::Psindex
    )
}

pub(crate) fn keyed_root_order(organization: DatabaseOrganization) -> bool {
    matches!(
        organization,
        DatabaseOrganization::Hidam
            | DatabaseOrganization::Hisam
            | DatabaseOrganization::Msdb
            | DatabaseOrganization::Phidam
            | DatabaseOrganization::Shisam
    )
}

/// Explicit resource bounds for one database engine instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

/// A secondary access path. Historical descriptors use `field` alone and
/// point to their source; extensions preserve that descriptor's serialized bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SecondaryIndexDefinition {
    pub name: String,
    pub source_segment: String,
    #[serde(alias = "source_field")]
    pub field: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_segment: Option<String>,
    pub unique: bool,
}

impl Serialize for SecondaryIndexDefinition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let extended = !self.additional_fields.is_empty() || self.target_segment.is_some();
        let mut state = serializer.serialize_struct(
            "SecondaryIndexDefinition",
            4 + usize::from(!self.additional_fields.is_empty())
                + usize::from(self.target_segment.is_some()),
        )?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("source_segment", &self.source_segment)?;
        // Prior descriptor readers ignore unknown fields. Omitting their required
        // `field` key forces them to reject an extended image instead of silently
        // using its first source field and returning its source occurrence.
        state.serialize_field(if extended { "source_field" } else { "field" }, &self.field)?;
        if !self.additional_fields.is_empty() {
            state.serialize_field("additional_fields", &self.additional_fields)?;
        }
        if self.target_segment.is_some() {
            state.serialize_field("target_segment", &self.target_segment)?;
        }
        state.serialize_field("unique", &self.unique)?;
        state.end()
    }
}

impl SecondaryIndexDefinition {
    pub fn target_segment(&self) -> &str {
        self.target_segment
            .as_deref()
            .unwrap_or(&self.source_segment)
    }

    fn fields(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.field.as_str())
            .chain(self.additional_fields.iter().map(String::as_str))
    }
}

/// Immutable metadata consumed by the algorithm foundation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DatabaseDefinition {
    /// Optional complete GSAM application format; absence preserves fixed-only compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gsam_format: Option<mainframe_env_host_api::ImsGsamFormat>,
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
#[serde(deny_unknown_fields)]
struct HeldRecord {
    id: RecordId,
    version: u64,
}

/// Position and parentage owned by one future DB PCB adapter.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PcbPosition {
    current: Option<RecordId>,
    parentage: Option<RecordId>,
    held: Option<HeldRecord>,
    after_end: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary: Option<SecondaryPosition>,
    /// Original missing-occurrence boundary, not the ordinary-order predecessor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_restart: Option<Box<crate::recovery::SavedSecondaryPosition>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SecondaryPosition {
    index: String,
    source: RecordId,
}

impl PcbPosition {
    pub fn set_current(&mut self, id: RecordId) {
        self.secondary = None;
        self.secondary_restart = None;
        self.current = Some(id);
        self.parentage = Some(id);
        self.held = None;
        self.after_end = false;
    }

    pub fn current(&self) -> Option<RecordId> {
        self.current
    }

    pub fn parentage(&self) -> Option<RecordId> {
        self.parentage
    }

    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    pub(crate) fn secondary_index(&self) -> Option<&str> {
        self.secondary_restart
            .as_ref()
            .map(|saved| saved.index.as_str())
            .or_else(|| {
                self.secondary
                    .as_ref()
                    .map(|selected| selected.index.as_str())
            })
    }

    pub(crate) fn has_secondary_restart_boundary(&self) -> bool {
        self.secondary_restart.is_some()
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
    /// Issued logical address identity. Absent in retained historical images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gsam_address: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_checkpoint_identity: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct LogicalLink {
    pub(crate) child: RecordId,
    pub(crate) parent_database: String,
    pub(crate) parent_segment: String,
    pub(crate) parent: RecordId,
    pub(crate) paired: bool,
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
    logical_links: BTreeSet<LogicalLink>,
    next_id: u64,
    revision: u64,
}

/// Durable row payload; byte-valued index keys are rebuilt from record data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DatabaseEngineImage {
    definition: DatabaseDefinition,
    records: Vec<Record>,
    roots: Vec<RecordId>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    logical_links: BTreeSet<LogicalLink>,
    next_id: u64,
    revision: u64,
}

mod definition;
mod gsam;
mod gsam_checkpoint;
pub(crate) mod gsam_format;
mod logical;
mod navigation;
mod secondary;
mod secondary_checkpoint;
mod ssa;
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
            logical_links: BTreeSet::new(),
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

    pub fn image(&self) -> DatabaseEngineImage {
        DatabaseEngineImage {
            definition: self.definition.clone(),
            records: self.records.values().cloned().collect(),
            roots: self.roots.clone(),
            logical_links: self.logical_links.clone(),
            next_id: self.next_id,
            revision: self.revision,
        }
    }

    pub fn restore(
        image: DatabaseEngineImage,
        limits: EngineLimits,
    ) -> Result<Self, EngineProblem> {
        let mut engine = Self::new(image.definition, limits)?;
        if image.records.len() > limits.max_records || image.roots.len() > limits.max_records {
            return Err(EngineProblem::LimitExceeded);
        }
        for record in image.records {
            if engine.records.insert(record.id, record).is_some() {
                return Err(EngineProblem::InvalidData);
            }
        }
        engine.roots = image.roots;
        engine.logical_links = image.logical_links;
        engine.next_id = image.next_id;
        engine.revision = image.revision;
        for (id, record) in &engine.records {
            for (name, value) in engine.index_values(&record.segment, &record.data)? {
                if let Some(value) = value {
                    engine
                        .indexes
                        .get_mut(&name)
                        .ok_or(EngineProblem::InvalidData)?
                        .entry(value)
                        .or_default()
                        .insert(*id);
                }
            }
        }
        engine.validate_image()?;
        Ok(engine)
    }

    /// Check a deserialized image before it is admitted from durable storage.
    pub fn validate_image(&self) -> Result<(), EngineProblem> {
        validate_definition(&self.definition, self.limits)?;
        self.validate_gsam_addresses()?;
        self.validate_secondary_checkpoint_identities()?;
        if self.records.len() > self.limits.max_records
            || self.next_id == 0
            || self.records.keys().any(|id| id.0 >= self.next_id)
            || self.roots.len() > self.limits.max_records
        {
            return Err(EngineProblem::InvalidData);
        }
        let mut roots = BTreeSet::new();
        for root in &self.roots {
            if !roots.insert(*root)
                || self
                    .records
                    .get(root)
                    .is_none_or(|record| record.parent.is_some())
            {
                return Err(EngineProblem::InvalidData);
            }
        }
        let mut ordered_roots = self.roots.clone();
        let keyed = keyed_root_order(self.definition.organization);
        self.sort_ids(&mut ordered_roots, keyed);
        if ordered_roots != self.roots {
            return Err(EngineProblem::InvalidData);
        }
        let mut reachable = BTreeSet::new();
        let mut pending = self.roots.clone();
        while let Some(id) = pending.pop() {
            if !reachable.insert(id) {
                return Err(EngineProblem::InvalidData);
            }
            let record = self.records.get(&id).ok_or(EngineProblem::InvalidData)?;
            if record
                .children
                .iter()
                .any(|child| !self.records.contains_key(child))
            {
                return Err(EngineProblem::InvalidData);
            }
            let mut children = record.children.clone();
            self.sort_ids(&mut children, true);
            if children != record.children {
                return Err(EngineProblem::InvalidData);
            }
            for child in &record.children {
                let descendant = self.records.get(child).ok_or(EngineProblem::InvalidData)?;
                if descendant.parent != Some(id) {
                    return Err(EngineProblem::InvalidData);
                }
            }
            pending.extend(record.children.iter().copied());
        }
        if reachable.len() != self.records.len() {
            return Err(EngineProblem::InvalidData);
        }
        self.validate_logical_links()?;
        let mut rebuilt = self.clone();
        rebuilt.indexes = self
            .definition
            .secondary_indexes
            .iter()
            .map(|index| (index.name.clone(), BTreeMap::new()))
            .collect();
        for (id, record) in &self.records {
            if record.id != *id
                || id.0 == 0
                || record.version == 0
                || record.version > self.revision
                || record.data.len() > self.limits.max_segment_bytes
            {
                return Err(EngineProblem::InvalidData);
            }
            let segment = self.segment(&record.segment)?;
            self.validate_data(segment, &record.data)?;
            match record.parent {
                Some(parent) => {
                    let parent_record = self
                        .records
                        .get(&parent)
                        .ok_or(EngineProblem::InvalidData)?;
                    if segment.parent.as_deref() != Some(parent_record.segment.as_str())
                        || !parent_record.children.contains(id)
                    {
                        return Err(EngineProblem::InvalidData);
                    }
                }
                None if !roots.contains(id) => return Err(EngineProblem::InvalidData),
                None => {}
            }
            if record.children.len() > self.limits.max_children_per_parent
                || record
                    .children
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != record.children.len()
            {
                return Err(EngineProblem::InvalidData);
            }
            for (name, value) in self.index_values(&record.segment, &record.data)? {
                if let Some(value) = value {
                    rebuilt
                        .indexes
                        .get_mut(&name)
                        .ok_or(EngineProblem::InvalidData)?
                        .entry(value)
                        .or_default()
                        .insert(*id);
                }
            }
        }
        if rebuilt.indexes != self.indexes {
            return Err(EngineProblem::InvalidData);
        }
        for index in &self.definition.secondary_indexes {
            if index.unique
                && self.indexes[&index.name]
                    .values()
                    .any(|sources| sources.len() > 1)
            {
                return Err(EngineProblem::InvalidData);
            }
        }
        Ok(())
    }

    pub fn path_to(&self, id: RecordId) -> Result<Vec<RecordView>, EngineProblem> {
        self.record_path(id)
            .map(|path| path.into_iter().map(|id| self.view(id)).collect())
    }

    pub fn ordered_records(&self) -> Vec<RecordView> {
        self.hierarchy_order()
            .into_iter()
            .map(|id| self.view(id))
            .collect()
    }

    pub fn validate_position(&self, position: &PcbPosition) -> Result<(), EngineProblem> {
        self.validate_secondary_position(position)?;
        if position
            .current
            .is_some_and(|id| !self.records.contains_key(&id))
            || position
                .parentage
                .is_some_and(|id| !self.records.contains_key(&id))
            || position
                .held
                .is_some_and(|held| position.current != Some(held.id) || held.version == 0)
        {
            return Err(EngineProblem::InvalidData);
        }
        Ok(())
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
            &self.logical_links,
            indexes,
        ))
        .expect("bounded database state has an infallible JSON representation");
        Sha256::digest(material).into()
    }
}

#[cfg(test)]
mod secondary_tests;
#[cfg(test)]
mod tests;
