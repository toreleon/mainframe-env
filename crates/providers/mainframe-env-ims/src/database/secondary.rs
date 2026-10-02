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
        let Some(selected) = &position.secondary else {
            return Ok(());
        };
        if !position.valid_retained_shape() {
            return Err(EngineProblem::InvalidData);
        }
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
        let mut next = position.clone();
        next.held = None;
        let parent = if request.kind == ReadKind::NextInParent {
            let parent = next.parentage.ok_or(EngineProblem::ParentageRequired)?;
            self.validate_parent_path(parent, &physical)?;
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
        let mut started =
            request.kind == ReadKind::Unique || next.after_end || next.current.is_none();
        let mut found = None;
        'entries: for (key, sources) in entries {
            let mut sources = sources.iter().copied().collect::<Vec<_>>();
            sources.sort_by_key(|source| ranks[source]);
            for source in &sources {
                if request.kind == ReadKind::NextInParent
                    && next
                        .secondary
                        .as_ref()
                        .is_none_or(|selected| selected.index != name || selected.source != *source)
                {
                    continue;
                }
                let target = self.index_target(name, *source)?;
                for id in order
                    .iter()
                    .copied()
                    .filter(|id| *id == target || self.is_descendant(*id, target))
                {
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
                    let target_name = physical.target.as_deref().or_else(|| {
                        (request.kind == ReadKind::Unique).then_some(index.target_segment())
                    });
                    if visible(&self.records[&id].segment)
                        && self.matches(id, target_name, &physical.path)
                        && indexed.iter().all(|predicate| {
                            super::navigation::compare(key, &predicate.value, predicate.relation)
                        })
                    {
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
}
