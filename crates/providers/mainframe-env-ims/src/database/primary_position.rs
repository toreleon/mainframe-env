//! Private primary witnesses and order addresses. Selection stays in navigation.
use super::*;
use mainframe_env_host_api::ImsSsa;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyPart {
    pub segment: String,
    pub key: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Occurrence {
    id: RecordId,
    parent: Option<RecordId>,
    segment: String,
    key: Vec<u8>,
    observed_version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Edge {
    Node,
    Subtree,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Provenance {
    Examined,
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Boundary {
    BeforeFirst,
    After {
        path: Vec<KeyPart>,
        anchor_id: RecordId,
        edge: Edge,
        provenance: Provenance,
        captured_revision: u64,
        captured_next_id: u64,
    },
    Missing {
        path: Vec<KeyPart>,
        captured_revision: u64,
        captured_next_id: u64,
    },
    End,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrimarySearch {
    version: u8,
    metadata_digest: [u8; 32],
    observed_revision: u64,
    observed_next_id: u64,
    levels: [Option<Occurrence>; 3],
    boundary: Boundary,
    feedback_path: Vec<KeyPart>,
}

/// Selected metadata certifies uniqueness; the engine certifies live occurrences.
pub(crate) struct PrimaryPlan {
    pub(crate) digest: [u8; 32],
    pub(crate) ssas: Vec<ImsSsa>,
    pub(crate) mask: [bool; 2],
}

pub(super) struct Progress {
    search: PrimarySearch,
    pub stopped: bool,
}

/// Actual successful DLET's removed set and immutable root order address.
pub(crate) struct PrimaryDeletion {
    pub(crate) removed: BTreeSet<RecordId>,
    anchor: RecordId,
    path: Vec<KeyPart>,
    revision: u64,
    next_id: u64,
}

impl PcbPosition {
    pub(crate) fn clear_primary(&mut self) {
        self.primary_search = None;
    }
    pub(crate) fn primary_feedback_path(&self) -> Option<&[KeyPart]> {
        self.primary_search
            .as_ref()
            .map(|s| s.feedback_path.as_slice())
    }

    pub(crate) fn validate_primary_identity(&self, digest: [u8; 32]) -> bool {
        self.primary_search
            .as_ref()
            .is_none_or(|s| s.metadata_digest == digest)
    }

    pub(crate) fn primary_parent(&self, segment: &str) -> Option<RecordId> {
        self.primary_search
            .as_ref()?
            .levels
            .iter()
            .flatten()
            .find(|o| o.segment == segment)
            .map(|o| o.id)
    }

    pub(crate) fn consume_primary_deletion(&mut self, deletion: &PrimaryDeletion, selected: bool) {
        let Some(search) = &mut self.primary_search else {
            return;
        };
        if let Some(first) = search
            .levels
            .iter()
            .position(|o| o.as_ref().is_some_and(|o| deletion.removed.contains(&o.id)))
        {
            search.levels[first..].fill(None);
        }
        if selected
            || matches!(&search.boundary, Boundary::After {anchor_id, provenance: Provenance::Examined, ..} if deletion.removed.contains(anchor_id))
        {
            search.boundary = Boundary::After {
                path: deletion.path.clone(),
                anchor_id: deletion.anchor,
                edge: Edge::Subtree,
                provenance: Provenance::Deleted,
                captured_revision: deletion.revision,
                captured_next_id: deletion.next_id,
            };
        }
        // Feedback is the last actual retrieval/ISRT snapshot, not today's live
        // certified levels. DLET never fabricates another key-feedback call.
        search.observed_revision = deletion.revision;
        search.observed_next_id = deletion.next_id;
        if self
            .current
            .is_some_and(|id| deletion.removed.contains(&id))
        {
            self.current = None;
        }
        if self
            .parentage
            .is_some_and(|id| deletion.removed.contains(&id))
        {
            self.parentage = None;
        }
        if self.held.is_some_and(|h| deletion.removed.contains(&h.id)) {
            self.held = None;
        }
    }
}

impl DatabaseEngine {
    pub(super) fn primary_deletion(
        &self,
        anchor: RecordId,
        removed: BTreeSet<RecordId>,
        revision: u64,
    ) -> Result<Option<PrimaryDeletion>, EngineProblem> {
        if !self.primary_shape() {
            return Ok(None);
        }
        Ok(Some(PrimaryDeletion {
            removed,
            anchor,
            path: self.key_path(anchor)?,
            revision,
            next_id: self.next_id,
        }))
    }

    pub(super) fn primary_replaced(&self, position: &mut PcbPosition, id: RecordId) {
        if let Some(s) = &mut position.primary_search {
            for occurrence in s.levels.iter_mut().flatten().filter(|o| o.id == id) {
                occurrence.observed_version = self.records[&id].version;
            }
            s.observed_revision = self.revision;
            s.observed_next_id = self.next_id;
        }
    }

    pub(crate) fn primary_inserted(
        &self,
        position: &mut PcbPosition,
        digest: [u8; 32],
        id: RecordId,
        old_parent: Option<RecordId>,
    ) -> Result<(), EngineProblem> {
        let path = self.record_path(id)?;
        let plan = PrimaryPlan {
            digest,
            ssas: vec![],
            mask: [false; 2],
        };
        let read = ReadRequest {
            kind: ReadKind::Unique,
            target: None,
            path: vec![],
            hold: false,
        };
        let mut progress = self.primary_progress(position, &plan, &read)?;
        self.primary_accept_prefix(&mut progress, &path, path.len())?;
        self.primary_examined(&mut progress, id)?;
        position.current = Some(id);
        position.held = None;
        position.parentage = old_parent.filter(|parent| self.is_descendant(id, *parent));
        self.primary_finish(position, progress, &read, Some(id), &plan);
        Ok(())
    }

    pub(crate) fn invalidate_primary_reinsertion(
        &self,
        position: &mut PcbPosition,
        id: RecordId,
    ) -> Result<(), EngineProblem> {
        let path = self.key_path(id)?;
        if position
            .primary_search
            .as_ref()
            .is_some_and(|s| match &s.boundary {
                Boundary::After {
                    path: old,
                    provenance: Provenance::Deleted,
                    ..
                }
                | Boundary::Missing { path: old, .. } => old == &path,
                _ => false,
            })
        {
            position.primary_search = None;
        }
        Ok(())
    }
    pub(crate) fn primary_shape(&self) -> bool {
        self.definition.organization == DatabaseOrganization::Hidam
            && self.logical_links.is_empty()
            && self.definition.segments.iter().all(|s| {
                s.min_length == s.max_length
                    && s.key_field.is_some()
                    && self.segment_path(&s.name).is_ok_and(|p| p.len() <= 3)
            })
            && self
                .definition
                .segments
                .iter()
                .any(|s| self.segment_path(&s.name).is_ok_and(|p| p.len() == 3))
    }

    pub(super) fn key_part(&self, id: RecordId) -> Result<KeyPart, EngineProblem> {
        let r = self.records.get(&id).ok_or(EngineProblem::InvalidData)?;
        let s = self.segment(&r.segment)?;
        Ok(KeyPart {
            segment: r.segment.clone(),
            key: navigation::field_value(
                s,
                s.key_field.as_deref().ok_or(EngineProblem::InvalidData)?,
                &r.data,
            )
            .ok_or(EngineProblem::InvalidData)?,
        })
    }

    fn key_path(&self, id: RecordId) -> Result<Vec<KeyPart>, EngineProblem> {
        self.record_path(id)?
            .into_iter()
            .map(|id| self.key_part(id))
            .collect()
    }

    fn valid_key_path(&self, path: &[KeyPart], empty: bool) -> bool {
        if path.len() > 3 || (!empty && path.is_empty()) {
            return false;
        }
        path.iter().enumerate().all(|(i, part)| {
            self.segment(&part.segment).is_ok_and(|s| {
                s.parent.as_deref() == i.checked_sub(1).map(|j| path[j].segment.as_str())
                    && s.key_field
                        .as_deref()
                        .and_then(|key| s.fields.iter().find(|f| f.name == key))
                        .is_some_and(|f| {
                            f.length == part.key.len()
                                && f.length > 0
                                && f.length <= self.limits.max_segment_bytes
                        })
            })
        })
    }

    pub(crate) fn validate_primary_position(
        &self,
        position: &PcbPosition,
        live: bool,
    ) -> Result<(), EngineProblem> {
        let Some(s) = &position.primary_search else {
            return Ok(());
        };
        let bad = || EngineProblem::InvalidData;
        let bound = self
            .limits
            .max_segment_bytes
            .checked_mul(45)
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(bad)?;
        if !self.primary_shape()
            || s.version != 1
            || s.observed_next_id == 0
            || position.secondary.is_some()
            || position.secondary_restart.is_some()
            || position.after_end
            || !self.valid_key_path(&s.feedback_path, true)
            || serde_json::to_vec(s).map_err(|_| bad())?.len() > bound
            || live && (s.observed_revision > self.revision || s.observed_next_id > self.next_id)
        {
            return Err(bad());
        }
        let mut path = Vec::new();
        let mut parent = None;
        let mut gap = false;
        for occurrence in &s.levels {
            let Some(o) = occurrence else {
                gap = true;
                continue;
            };
            if gap
                || o.id.0 == 0
                || o.id.0 >= s.observed_next_id
                || o.observed_version == 0
                || o.observed_version > s.observed_revision
                || o.parent != parent
            {
                return Err(bad());
            }
            path.push(KeyPart {
                segment: o.segment.clone(),
                key: o.key.clone(),
            });
            if !self.valid_key_path(&path, false) {
                return Err(bad());
            }
            if live
                && self.records.get(&o.id).is_none_or(|r| {
                    r.parent != o.parent
                        || r.segment != o.segment
                        || r.version < o.observed_version
                        || !self.key_part(o.id).is_ok_and(|p| p.key == o.key)
                })
            {
                return Err(bad());
            }
            parent = Some(o.id);
        }
        // A historical feedback snapshot may survive removal of its occurrence.
        // Intrinsic widths/topology above remain required; it is never a live level.
        let captures = match &s.boundary {
            Boundary::BeforeFirst | Boundary::End => None,
            Boundary::After {
                path,
                anchor_id,
                provenance,
                captured_revision,
                captured_next_id,
                ..
            } => {
                if !self.valid_key_path(path, false)
                    || anchor_id.0 == 0
                    || anchor_id.0 >= *captured_next_id
                    || live
                        && match provenance {
                            Provenance::Examined => !self
                                .key_path(*anchor_id)
                                .is_ok_and(|actual| actual == *path),
                            Provenance::Deleted => self.records.contains_key(anchor_id),
                        }
                {
                    return Err(bad());
                }
                Some((*captured_revision, *captured_next_id))
            }
            Boundary::Missing {
                path,
                captured_revision,
                captured_next_id,
            } => {
                if !self.valid_key_path(path, false) {
                    return Err(bad());
                }
                Some((*captured_revision, *captured_next_id))
            }
        };
        if captures.is_some_and(|(revision, next)| {
            next == 0 || revision > s.observed_revision || next > s.observed_next_id
        }) {
            return Err(bad());
        }
        Ok(())
    }

    pub(crate) fn validate_primary_plan(
        &self,
        position: &PcbPosition,
        plan: &PrimaryPlan,
    ) -> Result<(), EngineProblem> {
        if !self.primary_shape()
            || !position.valid_retained_shape()
            || position.secondary.is_some()
            || position.secondary_restart.is_some()
            || !position.validate_primary_identity(plan.digest)
        {
            return Err(EngineProblem::Unsupported);
        }
        self.validate_primary_position(position, true)?;
        for (level, active) in plan.mask.iter().enumerate() {
            if *active
                && position
                    .primary_search
                    .as_ref()
                    .and_then(|s| s.levels[level].as_ref())
                    .is_none_or(|o| {
                        plan.ssas
                            .get(level)
                            .is_none_or(|ssa| ssa.segment != o.segment)
                    })
            {
                return Err(EngineProblem::Unsupported);
            }
        }
        Ok(())
    }

    pub(super) fn primary_progress(
        &self,
        position: &PcbPosition,
        plan: &PrimaryPlan,
        request: &ReadRequest,
    ) -> Result<Progress, EngineProblem> {
        let boundary = if request.kind == ReadKind::Unique {
            Boundary::BeforeFirst
        } else {
            position
                .primary_search
                .as_ref()
                .map_or(Boundary::BeforeFirst, |s| s.boundary.clone())
        };
        let mut progress = Progress {
            search: PrimarySearch {
                version: 1,
                metadata_digest: plan.digest,
                observed_revision: self.revision,
                observed_next_id: self.next_id,
                levels: [None, None, None],
                boundary,
                feedback_path: vec![],
            },
            stopped: false,
        };
        // These are live, already certified U/V qualifications or the actual
        // validated GNP parent frame, never legacy returned-current ancestry.
        let frame = if request.kind == ReadKind::NextInParent {
            position.parentage
        } else {
            plan.mask.iter().rposition(|active| *active).and_then(|i| {
                position.primary_search.as_ref()?.levels[i]
                    .as_ref()
                    .map(|o| o.id)
            })
        };
        if let Some(id) = frame {
            let path = self.record_path(id)?;
            self.primary_accept_prefix(&mut progress, &path, path.len())?;
        }
        Ok(progress)
    }

    pub(super) fn primary_start(
        &self,
        position: &PcbPosition,
        plan: &PrimaryPlan,
        order: &[RecordId],
    ) -> Result<Option<usize>, EngineProblem> {
        let Some(s) = &position.primary_search else {
            return Ok(None);
        };
        let constrained = plan
            .mask
            .iter()
            .rposition(|active| *active)
            .and_then(|i| s.levels[i].as_ref());
        let anchor = match &s.boundary {
            Boundary::After {
                anchor_id,
                provenance: Provenance::Examined,
                ..
            } => Some(*anchor_id),
            Boundary::BeforeFirst => return Ok(Some(0)),
            Boundary::End => return Ok(Some(order.len())),
            Boundary::Missing { path, .. }
            | Boundary::After {
                path,
                provenance: Provenance::Deleted,
                ..
            } => {
                if let Some(parent) = constrained {
                    let parent_path = self.key_path(parent.id)?;
                    if !path.starts_with(&parent_path) {
                        return Ok(Some(
                            order
                                .iter()
                                .position(|id| *id == parent.id)
                                .ok_or(EngineProblem::InvalidData)?
                                + 1,
                        ));
                    }
                }
                // The existing primary loop compares the keyed gap; no
                // predecessor query or second traversal supplies witnesses.
                return Ok(Some(0));
            }
        };
        if let Some(parent) = constrained
            && anchor.is_some_and(|id| id != parent.id && !self.is_descendant(id, parent.id))
        {
            return Ok(Some(
                order
                    .iter()
                    .position(|id| *id == parent.id)
                    .ok_or(EngineProblem::InvalidData)?
                    + 1,
            ));
        }
        Ok(Some(
            anchor
                .and_then(|id| order.iter().position(|candidate| *candidate == id))
                .ok_or(EngineProblem::InvalidData)?
                + 1,
        ))
    }

    pub(super) fn primary_accept_prefix(
        &self,
        progress: &mut Progress,
        path: &[RecordId],
        count: usize,
    ) -> Result<(), EngineProblem> {
        for (level, id) in path.iter().take(count).enumerate() {
            let r = &self.records[id];
            if progress.search.levels[level]
                .as_ref()
                .is_none_or(|o| o.id != *id)
            {
                for slot in &mut progress.search.levels[level..] {
                    *slot = None;
                }
            }
            let part = self.key_part(*id)?;
            progress.search.levels[level] = Some(Occurrence {
                id: *id,
                parent: r.parent,
                segment: part.segment,
                key: part.key,
                observed_version: r.version,
            });
        }
        progress.search.feedback_path = progress
            .search
            .levels
            .iter()
            .flatten()
            .map(|o| KeyPart {
                segment: o.segment.clone(),
                key: o.key.clone(),
            })
            .collect();
        Ok(())
    }

    pub(super) fn primary_examined(
        &self,
        progress: &mut Progress,
        id: RecordId,
    ) -> Result<(), EngineProblem> {
        progress.search.boundary = Boundary::After {
            path: self.key_path(id)?,
            anchor_id: id,
            edge: Edge::Node,
            provenance: Provenance::Examined,
            captured_revision: self.revision,
            captured_next_id: self.next_id,
        };
        Ok(())
    }

    fn primary_order_address(
        &self,
        path: &[KeyPart],
    ) -> Result<Vec<(usize, Vec<u8>)>, EngineProblem> {
        path.iter()
            .map(|p| {
                Ok((
                    self.definition
                        .segments
                        .iter()
                        .position(|s| s.name == p.segment)
                        .ok_or(EngineProblem::InvalidData)?,
                    p.key.clone(),
                ))
            })
            .collect()
    }

    pub(super) fn primary_past_gap(
        &self,
        position: &PcbPosition,
        plan: &PrimaryPlan,
        request: &ReadRequest,
        id: RecordId,
    ) -> Result<bool, EngineProblem> {
        if request.kind == ReadKind::Unique {
            return Ok(true);
        }
        let Some(search) = &position.primary_search else {
            return Ok(true);
        };
        let (path, subtree) = match &search.boundary {
            Boundary::Missing { path, .. } => (path, false),
            Boundary::After {
                path,
                edge,
                provenance: Provenance::Deleted,
                ..
            } => (path, *edge == Edge::Subtree),
            _ => return Ok(true),
        };
        if let Some(parent) = plan
            .mask
            .iter()
            .rposition(|active| *active)
            .and_then(|i| search.levels[i].as_ref())
            && !path.starts_with(&self.key_path(parent.id)?)
        {
            return Ok(true);
        }
        let actual = self.primary_order_address(&self.key_path(id)?)?;
        let boundary = self.primary_order_address(path)?;
        if actual == boundary {
            return Err(EngineProblem::Unsupported);
        }
        Ok(actual > boundary && (!subtree || !actual.starts_with(&boundary)))
    }

    pub(super) fn primary_finish(
        &self,
        position: &mut PcbPosition,
        mut progress: Progress,
        request: &ReadRequest,
        selected: Option<RecordId>,
        plan: &PrimaryPlan,
    ) {
        if selected.is_none()
            && request.kind == ReadKind::Next
            && request.target.is_none()
            && request.path.is_empty()
        {
            position.primary_search = None;
            return;
        }
        if selected.is_none() && request.kind == ReadKind::Unique {
            let requested = plan.ssas.iter().map(|ssa| {
                let segment = self.segment(&ssa.segment).ok()?;
                let [predicate] = ssa.predicates.as_slice() else { return None; };
                if predicate.relation != mainframe_env_host_api::ImsSsaRelation::Equal
                    || !matches!(&predicate.field, mainframe_env_host_api::ImsSsaField::Named(name) if Some(name) == segment.key_field.as_ref())
                { return None; }
                Some(KeyPart { segment: ssa.segment.clone(), key: predicate.value.clone() })
            }).collect::<Option<Vec<_>>>();
            if let Some(path) = requested.filter(|p| !p.is_empty()) {
                progress.search.boundary = Boundary::Missing {
                    path,
                    captured_revision: self.revision,
                    captured_next_id: self.next_id,
                };
            }
        }
        position.after_end = false;
        position.primary_search = Some(Box::new(progress.search));
    }

    pub(super) fn primary_constraint(
        &self,
        position: &PcbPosition,
        plan: &PrimaryPlan,
        level: usize,
        path: &[RecordId],
    ) -> Option<RecordId> {
        if level >= 2 || !plan.mask[level] {
            return None;
        }
        let search = position.primary_search.as_ref()?;
        let occurrence = search.levels[level].as_ref()?;
        if level > 0
            && search.levels[level - 1]
                .as_ref()
                .is_none_or(|o| path.get(level - 1) != Some(&o.id))
        {
            return None;
        }
        Some(occurrence.id)
    }
}
