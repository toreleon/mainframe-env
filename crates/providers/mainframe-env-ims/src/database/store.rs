use super::navigation::field_value;
use super::*;

impl DatabaseEngine {
    pub fn insert(&mut self, request: InsertRequest) -> Result<RecordView, EngineProblem> {
        self.insert_internal(request, false, false)
    }

    /// Utility load can materialize index pointer rows; DL/I ISRT cannot.
    pub(crate) fn insert_loaded(
        &mut self,
        request: InsertRequest,
    ) -> Result<RecordView, EngineProblem> {
        self.insert_internal(request, true, false)
    }

    /// The generic adapter alone supplies validated SEQ M/default LAST admission.
    /// Historical engine insertion entry points continue to require unique keys.
    pub(crate) fn insert_nonunique_dependent_last(
        &mut self,
        request: InsertRequest,
        utility_load: bool,
    ) -> Result<RecordView, EngineProblem> {
        let segments = &self.definition.segments;
        if self.definition.organization != DatabaseOrganization::Hisam
            || segments.len() != 2
            || !self.definition.secondary_indexes.is_empty()
            || !self.logical_links.is_empty()
            || segments[0].parent.is_some()
            || segments[1].parent.as_deref() != Some(segments[0].name.as_str())
            || segments
                .iter()
                .any(|s| s.min_length != s.max_length || s.key_field.is_none())
            || request.segment != segments[1].name
            || request.parent.is_none_or(|id| {
                self.records
                    .get(&id)
                    .is_none_or(|r| r.segment != segments[0].name)
            })
        {
            return Err(EngineProblem::Unsupported);
        }
        self.insert_internal(request, utility_load, true)
    }

    /// ISRT preserves established ancestor parentage; it does not establish a
    /// new parent at the inserted occurrence. No retained position field changes.
    pub(crate) fn position_after_nonunique_dependent_insert(
        &self,
        position: &mut PcbPosition,
        id: RecordId,
    ) -> Result<(), EngineProblem> {
        let record = self.records.get(&id).ok_or(EngineProblem::InvalidRequest)?;
        let parentage = position
            .parentage
            .filter(|parent| record.parent == Some(*parent));
        position.set_current(id);
        position.parentage = parentage;
        Ok(())
    }

    fn insert_internal(
        &mut self,
        request: InsertRequest,
        utility_load: bool,
        nonunique_sequence: bool,
    ) -> Result<RecordView, EngineProblem> {
        if is_index_database(self.definition.organization) && !utility_load {
            return Err(EngineProblem::Unsupported);
        }
        if self.records.len() >= self.limits.max_records {
            return Err(EngineProblem::LimitExceeded);
        }
        let definition = self.segment(&request.segment)?.clone();
        self.validate_data(&definition, &request.data)?;
        match (&definition.parent, request.parent) {
            (None, None) => {}
            (Some(expected), Some(parent))
                if self
                    .records
                    .get(&parent)
                    .is_some_and(|record| &record.segment == expected) =>
            {
                let children = &self.records[&parent].children;
                if children.len() >= self.limits.max_children_per_parent {
                    return Err(EngineProblem::LimitExceeded);
                }
            }
            _ => return Err(EngineProblem::InvalidRequest),
        }
        if self.definition.organization == DatabaseOrganization::Gsam
            && (request.parent.is_some() || request.segment != self.definition.segments[0].name)
        {
            return Err(EngineProblem::Unsupported);
        }
        let key = self.primary_key(&definition, &request.data)?;
        if !nonunique_sequence
            && let Some(key) = &key
            && self.siblings(request.parent).iter().any(|id| {
                self.records
                    .get(id)
                    .filter(|record| record.segment == request.segment)
                    .and_then(|record| self.primary_key(&definition, &record.data).ok().flatten())
                    .as_ref()
                    == Some(key)
            })
        {
            return Err(EngineProblem::Duplicate);
        }
        let index_values = self.index_values(&request.segment, &request.data)?;
        self.validate_index_uniqueness(&index_values, None)?;
        let id = RecordId(self.next_id);
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(EngineProblem::LimitExceeded)?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(EngineProblem::LimitExceeded)?;
        let record = Record {
            id,
            segment: request.segment,
            parent: request.parent,
            data: request.data,
            children: Vec::new(),
            version: 1,
            gsam_address: None,
            secondary_checkpoint_identity: None,
        };
        self.records.insert(id, record);
        if let Some(parent) = request.parent {
            let mut children = self.records[&parent].children.clone();
            children.push(id);
            self.sort_ids(&mut children, true);
            self.records
                .get_mut(&parent)
                .ok_or(EngineProblem::InvalidRequest)?
                .children = children;
        } else {
            let mut roots = self.roots.clone();
            roots.push(id);
            let keyed = keyed_root_order(self.definition.organization);
            self.sort_ids(&mut roots, keyed);
            self.roots = roots;
        }
        for (name, value) in index_values {
            if let Some(value) = value {
                self.indexes
                    .get_mut(&name)
                    .ok_or(EngineProblem::InvalidDefinition)?
                    .entry(value)
                    .or_default()
                    .insert(id);
            }
        }
        self.next_id = next_id;
        self.revision = revision;
        Ok(self.view(id))
    }

    /// Execute GU/GN/GNP-style selection while updating the caller-owned PCB
    /// position only according to the selected navigation family.
    pub fn replace(
        &mut self,
        position: &mut PcbPosition,
        data: &[u8],
    ) -> Result<RecordView, EngineProblem> {
        if self.definition.organization == DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let held = self.current_hold(position)?;
        let record = self
            .records
            .get(&held.id)
            .ok_or(EngineProblem::StaleHold)?
            .clone();
        let definition = self.segment(&record.segment)?.clone();
        self.validate_data(&definition, data)?;
        if self.primary_key(&definition, &record.data)? != self.primary_key(&definition, data)? {
            return Err(EngineProblem::KeyChange);
        }
        let old_values = self.index_values(&record.segment, &record.data)?;
        let new_values = self.index_values(&record.segment, data)?;
        self.validate_index_uniqueness(&new_values, Some(record.id))?;
        let version = record
            .version
            .checked_add(1)
            .ok_or(EngineProblem::LimitExceeded)?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(EngineProblem::LimitExceeded)?;
        let mut indexes = self.indexes.clone();
        update_indexes(&mut indexes, record.id, &old_values, &new_values)?;
        let updated = self
            .records
            .get_mut(&record.id)
            .ok_or(EngineProblem::StaleHold)?;
        updated.data = data.to_vec();
        updated.version = version;
        if old_values != new_values {
            updated.secondary_checkpoint_identity = None;
        }
        self.indexes = indexes;
        self.revision = revision;
        position.held = Some(HeldRecord {
            id: record.id,
            version,
        });
        self.primary_replaced(position, record.id);
        if position.secondary.as_ref().is_some_and(|selected| {
            old_values
                .iter()
                .zip(&new_values)
                .any(|(old, new)| old.0 == selected.index && old != new)
        }) {
            position.parentage = None;
        }
        if position.secondary.as_ref().is_some_and(|selected| {
            selected.source == record.id
                && new_values
                    .iter()
                    .any(|(name, value)| name == &selected.index && value.is_none())
        }) {
            *position = PcbPosition::default();
        }
        Ok(self.view(record.id))
    }

    /// Physically delete the current held occurrence and all physical
    /// dependents, updating every secondary index atomically.
    pub fn delete(&mut self, position: &mut PcbPosition) -> Result<usize, EngineProblem> {
        self.delete_with_primary(position).map(|(count, _)| count)
    }

    pub(crate) fn delete_with_primary(
        &mut self,
        position: &mut PcbPosition,
    ) -> Result<(usize, Option<primary_position::PrimaryDeletion>), EngineProblem> {
        if self.definition.organization == DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let held = self.current_hold(position)?;
        let record = self
            .records
            .get(&held.id)
            .ok_or(EngineProblem::StaleHold)?
            .clone();
        let mut removed = BTreeSet::new();
        let mut pending = vec![record.id];
        while let Some(id) = pending.pop() {
            if removed.insert(id) {
                let current = self.records.get(&id).ok_or(EngineProblem::StaleHold)?;
                pending.extend(current.children.iter().copied());
                if removed.len() > self.limits.max_records {
                    return Err(EngineProblem::LimitExceeded);
                }
            }
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(EngineProblem::LimitExceeded)?;
        let deletion = self.primary_deletion(record.id, removed.clone(), revision)?;
        let mut records = self.records.clone();
        let mut roots = self.roots.clone();
        let mut indexes = self.indexes.clone();
        for id in &removed {
            let deleted = records.remove(id).ok_or(EngineProblem::StaleHold)?;
            let values = self.index_values(&deleted.segment, &deleted.data)?;
            remove_index_values(&mut indexes, *id, &values)?;
        }
        if let Some(parent) = record.parent {
            records
                .get_mut(&parent)
                .ok_or(EngineProblem::StaleHold)?
                .children
                .retain(|id| !removed.contains(id));
        } else {
            roots.retain(|id| !removed.contains(id));
        }
        self.records = records;
        self.roots = roots;
        self.indexes = indexes;
        self.logical_links
            .retain(|link| !removed.contains(&link.child));
        self.revision = revision;
        position.current = record.parent;
        position.held = None;
        position.after_end = false;
        if position.parentage.is_some_and(|id| removed.contains(&id)) {
            position.parentage = None;
        }
        if position
            .secondary
            .as_ref()
            .is_some_and(|selected| removed.contains(&selected.source))
        {
            *position = PcbPosition::default();
        }
        if let Some(deletion) = &deletion {
            position.consume_primary_deletion(deletion, true);
        }
        Ok((removed.len(), deletion))
    }

    /// Resolve an exact secondary-index value in current hierarchical order.
    pub(super) fn current_hold(&self, position: &PcbPosition) -> Result<HeldRecord, EngineProblem> {
        let held = position.held.ok_or(EngineProblem::HoldRequired)?;
        if position.current != Some(held.id)
            || self.records.get(&held.id).map(|record| record.version) != Some(held.version)
        {
            return Err(EngineProblem::StaleHold);
        }
        Ok(held)
    }

    pub(super) fn segment(&self, name: &str) -> Result<&SegmentDefinition, EngineProblem> {
        self.definition
            .segments
            .iter()
            .find(|segment| segment.name == name)
            .ok_or(EngineProblem::InvalidRequest)
    }

    pub(super) fn validate_data(
        &self,
        definition: &SegmentDefinition,
        data: &[u8],
    ) -> Result<(), EngineProblem> {
        if data.len() < definition.min_length
            || data.len() > definition.max_length
            || data.len() > self.limits.max_segment_bytes
        {
            Err(EngineProblem::InvalidData)
        } else {
            self.definition
                .gsam_format
                .as_ref()
                .map_or(Ok(()), |format| {
                    format
                        .validate_area(data, definition.min_length, definition.max_length)
                        .map_err(|_| EngineProblem::InvalidData)
                })
        }
    }

    pub(super) fn primary_key(
        &self,
        definition: &SegmentDefinition,
        data: &[u8],
    ) -> Result<Option<Vec<u8>>, EngineProblem> {
        definition
            .key_field
            .as_ref()
            .map(|name| field_value(definition, name, data).ok_or(EngineProblem::InvalidData))
            .transpose()
    }

    pub(super) fn siblings(&self, parent: Option<RecordId>) -> &[RecordId] {
        parent
            .and_then(|id| {
                self.records
                    .get(&id)
                    .map(|record| record.children.as_slice())
            })
            .unwrap_or(&self.roots)
    }

    pub(super) fn index_values(
        &self,
        segment: &str,
        data: &[u8],
    ) -> Result<IndexValues, EngineProblem> {
        let definition = self.segment(segment)?;
        self.definition
            .secondary_indexes
            .iter()
            .filter(|index| index.source_segment == segment)
            .map(|index| {
                Ok((
                    index.name.clone(),
                    self.index_value(index, definition, data)?,
                ))
            })
            .collect()
    }

    pub(super) fn validate_index_uniqueness(
        &self,
        values: &IndexValues,
        replacing: Option<RecordId>,
    ) -> Result<(), EngineProblem> {
        for (name, value) in values {
            let Some(value) = value else { continue };
            let definition = self
                .definition
                .secondary_indexes
                .iter()
                .find(|index| index.name == *name)
                .ok_or(EngineProblem::InvalidDefinition)?;
            if definition.unique
                && self.indexes[name]
                    .get(value)
                    .is_some_and(|ids| ids.iter().any(|id| Some(*id) != replacing))
            {
                return Err(EngineProblem::IndexConflict);
            }
        }
        Ok(())
    }

    pub(super) fn sort_ids(&self, ids: &mut [RecordId], keyed: bool) {
        if !keyed {
            return;
        }
        ids.sort_by(|left, right| {
            self.sort_value(*left)
                .cmp(&self.sort_value(*right))
                .then_with(|| left.cmp(right))
        });
    }

    pub(super) fn sort_value(&self, id: RecordId) -> (usize, Option<&[u8]>) {
        let record = &self.records[&id];
        let level = self
            .definition
            .segments
            .iter()
            .position(|segment| segment.name == record.segment)
            .unwrap_or(self.definition.segments.len());
        let key = self
            .definition
            .segments
            .get(level)
            .and_then(|segment| segment.key_field.as_ref().map(|name| (segment, name)))
            .and_then(|(segment, name)| segment.fields.iter().find(|field| &field.name == name))
            .and_then(|field| record.data.get(field.offset..field.offset + field.length));
        (level, key)
    }

    pub(super) fn view(&self, id: RecordId) -> RecordView {
        let record = &self.records[&id];
        RecordView {
            id,
            segment: record.segment.clone(),
            parent: record.parent,
            data: record.data.clone(),
        }
    }
}

fn update_indexes(
    indexes: &mut BTreeMap<String, BTreeMap<Vec<u8>, BTreeSet<RecordId>>>,
    id: RecordId,
    old_values: &IndexValues,
    new_values: &IndexValues,
) -> Result<(), EngineProblem> {
    remove_index_values(indexes, id, old_values)?;
    for (name, value) in new_values {
        if let Some(value) = value {
            indexes
                .get_mut(name)
                .ok_or(EngineProblem::InvalidDefinition)?
                .entry(value.clone())
                .or_default()
                .insert(id);
        }
    }
    Ok(())
}

fn remove_index_values(
    indexes: &mut BTreeMap<String, BTreeMap<Vec<u8>, BTreeSet<RecordId>>>,
    id: RecordId,
    values: &IndexValues,
) -> Result<(), EngineProblem> {
    for (name, value) in values {
        let Some(value) = value else { continue };
        let entries = indexes
            .get_mut(name)
            .ok_or(EngineProblem::InvalidDefinition)?;
        if let Some(ids) = entries.get_mut(value) {
            ids.remove(&id);
            if ids.is_empty() {
                entries.remove(value);
            }
        }
    }
    Ok(())
}
