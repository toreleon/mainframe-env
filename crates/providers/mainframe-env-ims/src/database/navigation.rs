use super::*;
use std::cmp::Ordering;

/// Call-local choice within the existing forward interval; never retained.
pub(super) enum Selection {
    First,
    Last,
}

impl PcbPosition {
    /// Check intrinsic retained PCB invariants without requiring a checkpoint's
    /// historical occurrences to exist in the current live database image.
    pub(crate) fn valid_retained_shape(&self) -> bool {
        !self.current.is_some_and(|id| id.0 == 0)
            && !self.parentage.is_some_and(|id| id.0 == 0)
            && !self
                .held
                .is_some_and(|held| held.version == 0 || self.current != Some(held.id))
            && (!self.after_end
                || (self.current.is_none() && self.parentage.is_none() && self.held.is_none()))
            && self.secondary.as_ref().is_none_or(|selected| {
                selected.source.0 != 0
                    && !selected.index.is_empty()
                    && self.current.is_some()
                    && !self.after_end
            })
            && self.secondary_restart.as_ref().is_none_or(|saved| {
                saved
                    .validate(crate::recovery::RecoveryLimits::default())
                    .is_ok()
                    && self.parentage.is_none()
                    && self.held.is_none()
                    && !self.after_end
                    && self
                        .secondary
                        .as_ref()
                        .is_none_or(|p| p.index == saved.index)
            })
    }
}

impl DatabaseEngine {
    pub fn read(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
    ) -> Result<RecordView, EngineProblem> {
        self.read_visible(position, request, |_| true)
    }

    /// The PCB adapter filters candidates before selection, preserving the same
    /// navigation and failure-position authority as an unrestricted engine read.
    pub(crate) fn read_visible(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
        visible: impl Fn(&str) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        if request.hold && self.definition.organization == DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        self.validate_read(request)?;
        self.read_matching(position, request, |id| {
            let target = request.target.as_deref().or_else(|| {
                (request.kind == ReadKind::Unique)
                    .then_some(self.definition.segments[0].name.as_str())
            });
            visible(&self.records[&id].segment) && self.matches(id, target, &request.path)
        })
    }

    pub(super) fn read_matching(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
        matches: impl Fn(RecordId) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.read_matching_selected(position, request, Selection::First, matches)
    }

    pub(super) fn read_matching_selected(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
        selection: Selection,
        matches: impl Fn(RecordId) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        let mut next = position.clone();
        next.held = None;
        let order = self.hierarchy_order();
        let selected = match request.kind {
            ReadKind::Unique => order.iter().copied().find(|id| matches(*id)),
            ReadKind::Next => {
                let start = if next.after_end {
                    0
                } else {
                    next.current
                        .and_then(|current| order.iter().position(|id| *id == current))
                        .map_or(0, |index| index + 1)
                };
                order[start..].iter().copied().find(|id| matches(*id))
            }
            ReadKind::NextInParent => {
                let parent = next.parentage.ok_or(EngineProblem::ParentageRequired)?;
                self.validate_parent_path(parent, request)?;
                let parent_index = order
                    .iter()
                    .position(|id| *id == parent)
                    .ok_or(EngineProblem::ParentageRequired)?;
                let stop = order[parent_index + 1..]
                    .iter()
                    .position(|id| !self.is_descendant(*id, parent))
                    .map_or(order.len(), |offset| parent_index + 1 + offset);
                let start = next
                    .current
                    .and_then(|current| {
                        order[parent_index + 1..stop]
                            .iter()
                            .position(|id| *id == current)
                    })
                    .map_or(parent_index + 1, |offset| parent_index + 2 + offset);
                let mut candidates = order[start..stop].iter().copied();
                match selection {
                    Selection::First => candidates.find(|id| matches(*id)),
                    Selection::Last => {
                        candidates.fold(
                            None,
                            |selected, id| {
                                if matches(id) { Some(id) } else { selected }
                            },
                        )
                    }
                }
            }
        };
        let Some(id) = selected else {
            match request.kind {
                ReadKind::Unique => next.parentage = None,
                ReadKind::Next => {
                    next.current = None;
                    next.parentage = None;
                    next.after_end = true;
                }
                ReadKind::NextInParent => {}
            }
            *position = next;
            return Err(
                if request.kind == ReadKind::Next
                    && request.target.is_none()
                    && request.path.is_empty()
                {
                    EngineProblem::EndOfDatabase
                } else {
                    EngineProblem::NotFound
                },
            );
        };
        next.current = Some(id);
        next.after_end = false;
        if request.kind != ReadKind::NextInParent {
            next.parentage = Some(id);
        }
        if request.hold {
            next.held = Some(HeldRecord {
                id,
                version: self.records[&id].version,
            });
        }
        *position = next;
        Ok(self.view(id))
    }

    /// Resolve pointer entries to target occurrences, preserving duplicate
    /// pointers when distinct sources index the same target.
    pub fn lookup_index(&self, name: &str, value: &[u8]) -> Result<Vec<RecordView>, EngineProblem> {
        let entries = self
            .indexes
            .get(name)
            .ok_or(EngineProblem::InvalidRequest)?;
        let ids = entries.get(value).cloned().unwrap_or_default();
        self.hierarchy_order()
            .into_iter()
            .filter(|id| ids.contains(id))
            .map(|id| self.index_target(name, id).map(|target| self.view(target)))
            .collect()
    }

    pub(super) fn validate_read(&self, request: &ReadRequest) -> Result<(), EngineProblem> {
        let predicates = request
            .path
            .iter()
            .try_fold(0usize, |count, selector| {
                count.checked_add(selector.predicates.len())
            })
            .ok_or(EngineProblem::LimitExceeded)?;
        if predicates > self.limits.max_predicates {
            return Err(EngineProblem::LimitExceeded);
        }
        if request.path.is_empty() {
            if let Some(target) = &request.target {
                self.segment(target)?;
            }
            return Ok(());
        }
        let target = request
            .target
            .as_deref()
            .ok_or(EngineProblem::InvalidRequest)?;
        let expected_path = self.segment_path(target)?;
        let mut last_level = None;
        for selector in &request.path {
            let level = expected_path
                .iter()
                .position(|name| name == &selector.segment)
                .ok_or(EngineProblem::InvalidRequest)?;
            if last_level.is_some_and(|prior| level <= prior) {
                return Err(EngineProblem::InvalidRequest);
            }
            let definition = self.segment(&selector.segment)?;
            for predicate in &selector.predicates {
                if predicate.value.len() > self.limits.max_segment_bytes
                    || !definition
                        .fields
                        .iter()
                        .any(|field| field.name == predicate.field)
                {
                    return Err(EngineProblem::InvalidRequest);
                }
            }
            last_level = Some(level);
        }
        Ok(())
    }

    pub(super) fn validate_parent_path(
        &self,
        parent: RecordId,
        request: &ReadRequest,
    ) -> Result<(), EngineProblem> {
        let parent_path = self.record_path(parent)?;
        if let Some(target) = &request.target {
            let target_path = self.segment_path(target)?;
            let parent_segment = &self.records[&parent].segment;
            let parent_level = target_path
                .iter()
                .position(|segment| segment == parent_segment)
                .ok_or(EngineProblem::PathMismatch)?;
            if parent_level + 1 >= target_path.len() {
                return Err(EngineProblem::PathMismatch);
            }
        }
        for selector in &request.path {
            if let Some(id) = parent_path
                .iter()
                .find(|id| self.records[id].segment == selector.segment)
                && !self.matches_selector(*id, selector)
            {
                return Err(EngineProblem::PathMismatch);
            }
        }
        Ok(())
    }

    pub(super) fn matches(
        &self,
        id: RecordId,
        target: Option<&str>,
        path: &[SegmentSelector],
    ) -> bool {
        let record = &self.records[&id];
        if target.is_some_and(|target| record.segment != target) {
            return false;
        }
        let Ok(record_path) = self.record_path(id) else {
            return false;
        };
        path.iter().all(|selector| {
            record_path
                .iter()
                .find(|candidate| self.records[candidate].segment == selector.segment)
                .is_some_and(|candidate| self.matches_selector(*candidate, selector))
        })
    }

    pub(super) fn matches_selector(&self, id: RecordId, selector: &SegmentSelector) -> bool {
        let record = &self.records[&id];
        let Ok(definition) = self.segment(&record.segment) else {
            return false;
        };
        selector.predicates.iter().all(|predicate| {
            optional_field_value(definition, &predicate.field, &record.data)
                .ok()
                .flatten()
                .is_some_and(|actual| compare(&actual, &predicate.value, predicate.relation))
        })
    }

    pub(super) fn hierarchy_order(&self) -> Vec<RecordId> {
        let mut order = Vec::with_capacity(self.records.len());
        for root in &self.roots {
            self.append_subtree(*root, &mut order);
        }
        order
    }

    pub(super) fn append_subtree(&self, id: RecordId, order: &mut Vec<RecordId>) {
        order.push(id);
        if let Some(record) = self.records.get(&id) {
            for child in &record.children {
                self.append_subtree(*child, order);
            }
        }
    }

    pub(super) fn is_descendant(&self, candidate: RecordId, ancestor: RecordId) -> bool {
        let mut current = self
            .records
            .get(&candidate)
            .and_then(|record| record.parent);
        while let Some(id) = current {
            if id == ancestor {
                return true;
            }
            current = self.records.get(&id).and_then(|record| record.parent);
        }
        false
    }

    pub(super) fn record_path(&self, id: RecordId) -> Result<Vec<RecordId>, EngineProblem> {
        let mut result = Vec::new();
        let mut current = Some(id);
        while let Some(candidate) = current {
            let record = self
                .records
                .get(&candidate)
                .ok_or(EngineProblem::NotFound)?;
            result.push(candidate);
            if result.len() > self.definition.segments.len() {
                return Err(EngineProblem::InvalidDefinition);
            }
            current = record.parent;
        }
        result.reverse();
        Ok(result)
    }

    pub(super) fn segment_path(&self, name: &str) -> Result<Vec<String>, EngineProblem> {
        let mut result = Vec::new();
        let mut current = Some(name);
        while let Some(candidate) = current {
            let segment = self.segment(candidate)?;
            result.push(segment.name.clone());
            if result.len() > self.definition.segments.len() {
                return Err(EngineProblem::InvalidDefinition);
            }
            current = segment.parent.as_deref();
        }
        result.reverse();
        Ok(result)
    }
}

pub(super) fn field_value(
    definition: &SegmentDefinition,
    name: &str,
    data: &[u8],
) -> Option<Vec<u8>> {
    optional_field_value(definition, name, data).ok().flatten()
}

pub(super) fn optional_field_value(
    definition: &SegmentDefinition,
    name: &str,
    data: &[u8],
) -> Result<Option<Vec<u8>>, EngineProblem> {
    let field = definition
        .fields
        .iter()
        .find(|field| field.name == name)
        .ok_or(EngineProblem::InvalidDefinition)?;
    let end = field
        .offset
        .checked_add(field.length)
        .ok_or(EngineProblem::InvalidDefinition)?;
    Ok(data.get(field.offset..end).map(<[u8]>::to_vec))
}

pub(super) fn compare(actual: &[u8], expected: &[u8], relation: Relation) -> bool {
    let ordering = actual.cmp(expected);
    match relation {
        Relation::Equal => ordering == Ordering::Equal,
        Relation::NotEqual => ordering != Ordering::Equal,
        Relation::LessThan => ordering == Ordering::Less,
        Relation::LessOrEqual => ordering != Ordering::Greater,
        Relation::GreaterThan => ordering == Ordering::Greater,
        Relation::GreaterOrEqual => ordering != Ordering::Less,
    }
}
