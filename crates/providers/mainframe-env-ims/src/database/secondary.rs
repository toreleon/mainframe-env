//! Secondary pointer construction and navigation share the physical engine and
//! caller-owned PCB position. Pointer identities are source occurrences; reads
//! return their target occurrences, never assume the two are interchangeable.

use super::navigation::optional_field_value;
use super::*;

impl DatabaseEngine {
    pub(super) fn index_value(
        &self,
        index: &SecondaryIndexDefinition,
        segment: &SegmentDefinition,
        data: &[u8],
    ) -> Result<Option<Vec<u8>>, EngineProblem> {
        let mut value = Vec::new();
        for name in index.fields() {
            let Some(bytes) = optional_field_value(segment, name, data)? else {
                // Retain historical single optional-field behavior. A partial
                // composite has no defined NULLVAL/exit contract in this shape.
                return if index.additional_fields.is_empty() {
                    Ok(None)
                } else {
                    Err(EngineProblem::InvalidData)
                };
            };
            value.extend(bytes);
        }
        Ok(Some(value))
    }

    pub(super) fn index_target(
        &self,
        name: &str,
        source: RecordId,
    ) -> Result<RecordId, EngineProblem> {
        let index = self
            .definition
            .secondary_indexes
            .iter()
            .find(|index| index.name == name)
            .ok_or(EngineProblem::InvalidRequest)?;
        let record = self
            .records
            .get(&source)
            .ok_or(EngineProblem::InvalidData)?;
        if record.segment != index.source_segment {
            return Err(EngineProblem::InvalidData);
        }
        self.record_path(source)?
            .into_iter()
            .find(|id| self.records[id].segment == index.target_segment())
            .ok_or(EngineProblem::InvalidData)
    }

    /// The metadata currently represents physical SENSEG paths. A selected
    /// nonroot target requires aliases and an inverted hierarchy it cannot own.
    pub(crate) fn validate_secondary_navigation(&self, name: &str) -> Result<(), EngineProblem> {
        let index = self
            .definition
            .secondary_indexes
            .iter()
            .find(|index| index.name == name)
            .ok_or(EngineProblem::InvalidRequest)?;
        if self.segment(index.target_segment())?.parent.is_some()
            || self.definition.organization == DatabaseOrganization::Dedb
            || index.fields().any(|name| {
                self.segment(&index.source_segment).is_ok_and(|source| {
                    source
                        .fields
                        .iter()
                        .find(|field| field.name == name)
                        .is_none_or(|field| field.offset + field.length > source.min_length)
                })
            })
        {
            return Err(EngineProblem::Unsupported);
        }
        Ok(())
    }

    pub(super) fn validate_secondary_position(
        &self,
        position: &PcbPosition,
    ) -> Result<(), EngineProblem> {
        if !position.valid_retained_shape() {
            return Err(EngineProblem::InvalidData);
        }
        if let Some(saved) = &position.secondary_restart {
            self.validate_secondary_navigation(&saved.index)?;
            let index = self
                .definition
                .secondary_indexes
                .iter()
                .find(|index| index.name == saved.index)
                .ok_or(EngineProblem::InvalidData)?;
            if saved
                .source_path
                .last()
                .is_none_or(|(name, _)| name != &index.source_segment)
                || saved.source_path.first() != saved.current_path.first()
                || saved
                    .current_path
                    .first()
                    .is_none_or(|(name, _)| name != index.target_segment())
            {
                return Err(EngineProblem::InvalidData);
            }
            self.checkpoint_order(&saved.source_path)?;
            self.checkpoint_order(&saved.current_path)?;
        }
        let Some(selected) = &position.secondary else {
            return Ok(());
        };
        let target = self.index_target(&selected.index, selected.source)?;
        let record = &self.records[&selected.source];
        if !self
            .index_values(&record.segment, &record.data)?
            .iter()
            .any(|(name, value)| name == &selected.index && value.is_some())
            || position
                .current
                .is_none_or(|id| id != target && !self.is_descendant(id, target))
            || position
                .parentage
                .is_some_and(|id| id != target && !self.is_descendant(id, target))
        {
            return Err(EngineProblem::InvalidData);
        }
        Ok(())
    }

    pub(crate) fn position_after_secondary_insert(
        &self,
        name: &str,
        position: &mut PcbPosition,
        inserted: RecordId,
    ) -> Result<(), EngineProblem> {
        let index = self
            .definition
            .secondary_indexes
            .iter()
            .find(|index| index.name == name)
            .ok_or(EngineProblem::InvalidRequest)?;
        let target = self
            .record_path(inserted)?
            .into_iter()
            .find(|id| self.records[id].segment == index.target_segment())
            .ok_or(EngineProblem::InvalidRequest)?;
        let source = if self.records[&inserted].segment == index.source_segment {
            Some(inserted)
        } else {
            position
                .secondary
                .as_ref()
                .filter(|selected| {
                    selected.index == name
                        && self
                            .index_target(name, selected.source)
                            .is_ok_and(|id| id == target)
                })
                .map(|selected| selected.source)
                .or_else(|| {
                    self.indexes[name]
                        .values()
                        .flat_map(|sources| sources.iter())
                        .copied()
                        .find(|source| {
                            self.index_target(name, *source)
                                .is_ok_and(|id| id == target)
                        })
                })
        }
        .ok_or(EngineProblem::NotFound)?;
        if position
            .parentage
            .is_some_and(|parent| !self.is_descendant(inserted, parent))
        {
            position.parentage = None;
        }
        position.current = Some(inserted);
        position.held = None;
        position.after_end = false;
        position.secondary_restart = None;
        position.secondary = Some(SecondaryPosition {
            index: name.into(),
            source,
        });
        self.validate_secondary_position(position)
    }

    /// Read in index-key order. Nonunique pointer ties use physical source
    /// order deterministically; that order is not a licensed tie-order promise.
    pub(crate) fn read_secondary_visible(
        &self,
        name: &str,
        position: &mut PcbPosition,
        request: &ReadRequest,
        visible: impl Fn(&str) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.validate_secondary_navigation(name)?;
        let index = self
            .definition
            .secondary_indexes
            .iter()
            .find(|index| index.name == name)
            .ok_or(EngineProblem::InvalidRequest)?;
        let mut physical = request.clone();
        let mut indexed = Vec::new();
        for selector in &mut physical.path {
            selector.predicates.retain(|predicate| {
                if selector.segment == index.target_segment() && predicate.field == name {
                    indexed.push(predicate.clone());
                    false
                } else {
                    true
                }
            });
        }
        self.validate_read(&physical)?;
        let count = request
            .path
            .iter()
            .try_fold(0usize, |count, selector| {
                count.checked_add(selector.predicates.len())
            })
            .ok_or(EngineProblem::LimitExceeded)?;
        if count > self.limits.max_predicates {
            return Err(EngineProblem::LimitExceeded);
        }
        if indexed
            .iter()
            .any(|predicate| predicate.value.len() > self.limits.max_segment_bytes)
        {
            return Err(EngineProblem::InvalidRequest);
        }
        self.read_secondary_matching(name, position, &physical, |id, key| {
            visible(&self.records[&id].segment)
                && self.matches(
                    id,
                    physical.target.as_deref().or_else(|| {
                        (request.kind == ReadKind::Unique).then_some(index.target_segment())
                    }),
                    &physical.path,
                )
                && indexed.iter().all(|predicate| {
                    super::navigation::compare(key, &predicate.value, predicate.relation)
                })
        })
    }

    /// One pointer/cursor authority for legacy and rich SSA selection. Callers
    /// validate operands before entering; matchers cannot change traversal state.
    pub(super) fn read_secondary_matching(
        &self,
        name: &str,
        position: &mut PcbPosition,
        request: &ReadRequest,
        matches: impl Fn(RecordId, &[u8]) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.read_secondary_occurrence(name, position, request, None, matches)
    }

    /// Checkpoint GU binds one proven pointer rather than choosing a tie anew.
    pub(super) fn read_secondary_occurrence(
        &self,
        name: &str,
        position: &mut PcbPosition,
        request: &ReadRequest,
        source_occurrence: Option<RecordId>,
        matches: impl Fn(RecordId, &[u8]) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.read_secondary_filtered(name, position, request, source_occurrence, None, matches)
    }

    /// Disjoint independent equality groups scan in SSA order, once per target
    /// per group. The same source-occurrence cursor and publication remain owned
    /// here; ordinary dependent/OR calls retain the full pointer sequence.
    pub(super) fn read_secondary_ordered_matching(
        &self,
        name: &str,
        position: &mut PcbPosition,
        request: &ReadRequest,
        independent_keys: Option<&[&[u8]]>,
        matches: impl Fn(RecordId, &[u8]) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.read_secondary_filtered(name, position, request, None, independent_keys, matches)
    }

    /// Both adapters constrain this one traversal; neither owns a second cursor.
    fn read_secondary_filtered(
        &self,
        name: &str,
        position: &mut PcbPosition,
        request: &ReadRequest,
        source_occurrence: Option<RecordId>,
        independent_keys: Option<&[&[u8]]>,
        matches: impl Fn(RecordId, &[u8]) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.validate_secondary_navigation(name)?;
        self.validate_read(request)?;
        let restart = position
            .secondary_restart
            .as_deref()
            .filter(|_| independent_keys.is_some() && request.kind != ReadKind::Unique);
        if let (Some(saved), Some(keys)) = (restart, independent_keys)
            && !keys.contains(&saved.search_key.as_slice())
        {
            return Err(EngineProblem::Unsupported);
        }
        if let Some(keys) = independent_keys
            && request.kind != ReadKind::Unique
            && position.current.is_some()
            && restart.is_none()
            && !self
                .selected_secondary_key(name, position)
                .is_ok_and(|key| keys.contains(&key))
        {
            // No group cursor is encoded for a prior pointer outside these
            // disjoint groups. Do not guess a new independent scan position.
            return Err(EngineProblem::Unsupported);
        }
        let mut next = position.clone();
        next.held = None;
        let parent = if request.kind == ReadKind::NextInParent {
            let parent = next.parentage.ok_or(EngineProblem::ParentageRequired)?;
            self.validate_parent_path(parent, request)?;
            Some(parent)
        } else {
            None
        };
        let order = self.hierarchy_order();
        let ranks = order
            .iter()
            .enumerate()
            .map(|(rank, id)| (*id, rank))
            .collect::<BTreeMap<_, _>>();
        let entries = self
            .indexes
            .get(name)
            .ok_or(EngineProblem::InvalidRequest)?;
        let mut started = restart.is_some()
            || request.kind == ReadKind::Unique
            || next.after_end
            || next.current.is_none();
        let mut found = None;
        let entries = independent_keys.map_or_else(
            || entries.iter().collect::<Vec<_>>(),
            |keys| {
                keys.iter()
                    .filter_map(|key| entries.get_key_value(*key))
                    .collect()
            },
        );
        'entries: for (key, sources) in entries {
            let mut targets = BTreeSet::new();
            let mut sources = sources.iter().copied().collect::<Vec<_>>();
            sources.sort_by_key(|source| ranks[source]);
            for source in &sources {
                if source_occurrence.is_some_and(|expected| expected != *source) {
                    continue;
                }
                if request.kind == ReadKind::NextInParent
                    && next
                        .secondary
                        .as_ref()
                        .is_none_or(|selected| selected.index != name || selected.source != *source)
                {
                    continue;
                }
                let target = self.index_target(name, *source)?;
                let first_pointer = independent_keys.is_none() || targets.insert(target);
                for id in order
                    .iter()
                    .copied()
                    .filter(|id| *id == target || self.is_descendant(*id, target))
                {
                    if let (Some(saved), Some(keys)) = (restart, independent_keys)
                        && !self.follows_secondary_restart(saved, keys, key, *source, id)?
                    {
                        continue;
                    }
                    if !started {
                        if next.current == Some(id)
                            && next.secondary.as_ref().is_some_and(|selected| {
                                selected.index == name && selected.source == *source
                            })
                        {
                            started = true;
                        }
                        continue;
                    }
                    if parent.is_some_and(|parent| !self.is_descendant(id, parent)) {
                        continue;
                    }
                    if first_pointer && matches(id, key) {
                        found = Some((id, *source));
                        break 'entries;
                    }
                }
            }
        }
        let Some((id, source)) = found else {
            match request.kind {
                ReadKind::Unique => next.parentage = None,
                ReadKind::Next => {
                    next = PcbPosition {
                        after_end: true,
                        ..PcbPosition::default()
                    };
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
        next.secondary_restart = None;
        next.secondary = Some(SecondaryPosition {
            index: name.into(),
            source,
        });
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

    pub(super) fn selected_secondary_key(
        &self,
        name: &str,
        position: &PcbPosition,
    ) -> Result<&[u8], EngineProblem> {
        let selected = position
            .secondary
            .as_ref()
            .filter(|p| p.index == name)
            .ok_or(EngineProblem::ParentageRequired)?;
        self.indexes
            .get(name)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|(_, sources)| sources.contains(&selected.source))
            })
            .map(|(key, _)| key.as_slice())
            .ok_or(EngineProblem::ParentageRequired)
    }
}
