//! Metadata-resolved SSA selection on the existing navigation authority.
use super::navigation::Selection;
use super::*;
use mainframe_env_host_api::{
    ImsSsa, ImsSsaBoolean, ImsSsaField, ImsSsaFieldResolver, ImsSsaRelation,
};

impl ImsSsaFieldResolver for DatabaseEngine {
    fn field_length(&self, segment: &str, field: &str) -> Option<usize> {
        self.segment(segment)
            .ok()?
            .fields
            .iter()
            .find(|f| f.name == field)
            .map(|f| f.length)
    }

    fn concatenated_key_length(&self, segment: &str) -> Option<usize> {
        self.segment_path(segment)
            .ok()?
            .iter()
            .try_fold(0usize, |n, name| {
                let s = self.segment(name).ok()?;
                n.checked_add(self.field_length(name, s.key_field.as_deref()?)?)
            })
    }
}

/// Selected XDFLD is a virtual field of its target, backed by the pointer key.
/// Physical fields and concatenated physical keys retain their existing catalog.
pub(crate) struct SsaFields<'a> {
    engine: &'a DatabaseEngine,
    secondary: Option<&'a str>,
}

impl<'a> SsaFields<'a> {
    fn selected_index_field(
        &self,
        segment: &str,
        field: &str,
    ) -> Option<&'a SecondaryIndexDefinition> {
        let index = self.secondary.and_then(|name| {
            self.engine
                .definition
                .secondary_indexes
                .iter()
                .find(|index| index.name == name)
        })?;
        (segment == index.target_segment() && field == index.name).then_some(index)
    }
}

impl ImsSsaFieldResolver for SsaFields<'_> {
    fn field_length(&self, segment: &str, field: &str) -> Option<usize> {
        if let Some(index) = self.selected_index_field(segment, field) {
            let source = self.engine.segment(&index.source_segment).ok()?;
            return index.fields().try_fold(0usize, |length, name| {
                length.checked_add(source.fields.iter().find(|f| f.name == name)?.length)
            });
        }
        self.engine.field_length(segment, field)
    }

    fn concatenated_key_length(&self, segment: &str) -> Option<usize> {
        self.engine.concatenated_key_length(segment)
    }
}

impl DatabaseEngine {
    /// Evaluate active frames in the sole navigation loop. Rejected data
    /// advances examination without replacing a previously satisfied prefix.
    pub(super) fn primary_step(
        &self,
        position: &PcbPosition,
        plan: &primary_position::PrimaryPlan,
        progress: &mut primary_position::Progress,
        id: RecordId,
    ) -> Result<bool, EngineProblem> {
        let path = self.record_path(id)?;
        if plan.ssas.is_empty() {
            self.primary_accept_prefix(progress, &path, path.len())?;
            self.primary_examined(progress, id)?;
            return Ok(true);
        }
        let mut accepted = 0;
        let mut fenced = true;
        for (level, candidate) in path.iter().enumerate() {
            let Some(ssa) = plan.ssas.get(level) else {
                return Ok(false);
            };
            let record = &self.records[candidate];
            if record.segment != ssa.segment {
                // A different dependent type is an end probe only when every
                // ancestor is fixed. Otherwise skip it and try later parents.
                if fenced {
                    progress.stopped = true;
                }
                return Ok(false);
            }
            let constraint = self.primary_constraint(position, plan, level, &path);
            let key_equal = self
                .segment(&ssa.segment)?
                .key_field
                .as_deref()
                .and_then(|key| {
                    ssa.predicates.iter().find(|p| {
                        matches!(&p.field, ImsSsaField::Named(name) if name == key)
                            && p.relation == ImsSsaRelation::Equal
                    })
                });
            if constraint.is_some_and(|expected| expected != *candidate) {
                if fenced {
                    progress.stopped = true;
                }
                return Ok(false);
            }
            if !self.matches_ssa(*candidate, ssa, None) {
                if fenced
                    && key_equal
                        .is_some_and(|p| self.key_part(*candidate).is_ok_and(|k| k.key > p.value))
                {
                    progress.stopped = true;
                    return Ok(false);
                }
                // A rejected parent was actually examined; its dependents were
                // not eligible. Do not replace B11 by a B13 data rejection.
                if level + 1 == path.len() {
                    self.primary_examined(progress, *candidate)?;
                }
                return Ok(false);
            }
            accepted += 1;
            self.primary_accept_prefix(progress, &path, accepted)?;
            fenced &= constraint.is_some() || key_equal.is_some();
        }
        self.primary_examined(progress, id)?;
        Ok(path.len() == plan.ssas.len())
    }

    /// Shared live-path fence for the finite F/L direct-child selections.
    /// Uniqueness/logical metadata and call context remain with the PCB adapter.
    /// The historical method name remains for existing L regression callers.
    pub(crate) fn validate_last_direct_child(
        &self,
        position: &PcbPosition,
        request: &ReadRequest,
        ssas: &[ImsSsa],
        secondary: Option<&str>,
    ) -> Result<(), EngineProblem> {
        let [root, child] = self.definition.segments.as_slice() else {
            return Err(EngineProblem::Unsupported);
        };
        let [ssa] = ssas else {
            return Err(EngineProblem::Unsupported);
        };
        if self.definition.organization != DatabaseOrganization::Hidam
            || secondary.is_some()
            || !self.logical_links().is_empty()
            || request.kind != ReadKind::NextInParent
            || root.parent.is_some()
            || child.parent.as_deref() != Some(root.name.as_str())
            || root.key_field.is_none()
            || child.key_field.is_none()
            || root.min_length != root.max_length
            || child.min_length != child.max_length
            || request.target.as_deref() != Some(child.name.as_str())
            || ssa.segment != child.name
            || ssa.concatenated_key.is_some()
            || !ssa.predicates.is_empty()
            || !ssa.connectors.is_empty()
            || ssa.commands.len() != 1
            || !matches!(ssa.commands[0].code, b'F' | b'L')
            || ssa.commands[0].code == b'F' && !self.definition.secondary_indexes.is_empty()
            || ssa.commands[0].subset_pointer.is_some()
            || !position.valid_retained_shape()
            || position.after_end
            || position.secondary.is_some()
            || position.secondary_restart.is_some()
        {
            return Err(EngineProblem::Unsupported);
        }
        let parent = position.parentage.ok_or(EngineProblem::Unsupported)?;
        let current = position.current.ok_or(EngineProblem::Unsupported)?;
        let parent_path = self
            .record_path(parent)
            .map_err(|_| EngineProblem::Unsupported)?;
        let current_path = self
            .record_path(current)
            .map_err(|_| EngineProblem::Unsupported)?;
        if parent_path.as_slice() != [parent]
            || self.records[&parent].segment != root.name
            || !(current_path.as_slice() == [parent]
                || current_path.as_slice() == [parent, current]
                    && self.records[&current].segment == child.name
                    && self.records[&current].parent == Some(parent))
            || position.held.is_some_and(|held| {
                held.id != current || held.version != self.records[&current].version
            })
        {
            return Err(EngineProblem::Unsupported);
        }
        Ok(())
    }

    pub(crate) fn ssa_fields<'a>(&'a self, secondary: Option<&'a str>) -> SsaFields<'a> {
        SsaFields {
            engine: self,
            secondary,
        }
    }

    pub(crate) fn read_ssas(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
        ssas: &[ImsSsa],
        secondary: Option<&str>,
        visible: impl Fn(&str) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.read_ssas_planned(position, request, ssas, secondary, visible, None)
    }

    pub(crate) fn read_ssas_planned(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
        ssas: &[ImsSsa],
        secondary: Option<&str>,
        visible: impl Fn(&str) -> bool,
        plan: Option<&primary_position::PrimaryPlan>,
    ) -> Result<RecordView, EngineProblem> {
        self.validate_ssas(request, ssas, secondary)?;
        let direct_child_selection = ssas
            .iter()
            .any(|ssa| ssa.commands.iter().any(|c| matches!(c.code, b'F' | b'L')));
        if direct_child_selection {
            self.validate_last_direct_child(position, request, ssas, secondary)?;
        }
        if request.kind == ReadKind::NextInParent {
            let parent = position.parentage.ok_or(EngineProblem::ParentageRequired)?;
            self.validate_parent_path(parent, request)?;
            let indexed = secondary
                .map(|name| {
                    self.selected_secondary_key(name, position)
                        .map(|key| (name, key))
                })
                .transpose()?;
            let path = self.record_path(parent)?;
            for ssa in ssas {
                if let Some(id) = path
                    .iter()
                    .find(|id| self.records[id].segment == ssa.segment)
                    && !self.matches_ssa(*id, ssa, indexed)
                {
                    return Err(EngineProblem::PathMismatch);
                }
            }
        }
        let matches = |id: RecordId, indexed: Option<(&str, &[u8])>| {
            visible(&self.records[&id].segment)
                && self.matches(id, request.target.as_deref(), &request.path)
                && self.record_path(id).is_ok_and(|path| {
                    ssas.iter().all(|ssa| {
                        path.iter()
                            .find(|id| self.records[id].segment == ssa.segment)
                            .is_some_and(|id| self.matches_ssa(*id, ssa, indexed))
                    })
                })
        };
        let view = if let Some(name) = secondary {
            // Distinct equality groups have disjoint keys, so the existing
            // source cursor also identifies their left-to-right scan position.
            let keys = ssas
                .iter()
                .find(|ssa| self.independent_index(ssa, secondary).is_some())
                .map(|ssa| {
                    ssa.predicates
                        .iter()
                        .map(|p| p.value.as_slice())
                        .collect::<Vec<_>>()
                });
            self.read_secondary_ordered_matching(
                name,
                position,
                request,
                keys.as_deref(),
                |id, key| matches(id, Some((name, key))),
            )
        } else {
            self.read_matching_progress(
                position,
                request,
                match ssas
                    .first()
                    .and_then(|ssa| ssa.commands.first())
                    .map(|c| c.code)
                {
                    Some(b'F') => Selection::FirstInParent,
                    Some(b'L') => Selection::Last,
                    _ => Selection::First,
                },
                |id| matches(id, None),
                plan,
            )
        }?;
        if let Some(parent) = ssas
            .iter()
            .find(|ssa| ssa.commands.iter().any(|c| c.code == b'P'))
        {
            position.parentage = self
                .record_path(view.id)?
                .into_iter()
                .find(|id| self.records[id].segment == parent.segment);
        }
        Ok(view)
    }

    pub(crate) fn validate_ssas(
        &self,
        request: &ReadRequest,
        ssas: &[ImsSsa],
        secondary: Option<&str>,
    ) -> Result<(), EngineProblem> {
        if let Some(name) = secondary {
            self.validate_secondary_navigation(name)?;
        }
        self.validate_read(request)?;
        if request.hold && self.definition.organization == DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let mut count = 0usize;
        let fields = self.ssa_fields(secondary);
        for ssa in ssas {
            let definition = self.segment(&ssa.segment)?;
            count += ssa.predicates.len();
            for predicate in &ssa.predicates {
                let length = match &predicate.field {
                    ImsSsaField::Named(name) => fields
                        .field_length(&ssa.segment, name)
                        .ok_or(EngineProblem::InvalidRequest)?,
                    ImsSsaField::Offset { position, length } => {
                        let start = usize::from(*position)
                            .checked_sub(1)
                            .ok_or(EngineProblem::InvalidRequest)?;
                        if start
                            .checked_add(usize::from(*length))
                            .is_none_or(|end| end > definition.max_length)
                        {
                            return Err(EngineProblem::InvalidRequest);
                        }
                        usize::from(*length)
                    }
                };
                if length != predicate.value.len() {
                    return Err(EngineProblem::InvalidRequest);
                }
            }
            // The supplemental HDAM/DEDB source requires randomizer anchors and
            // search termination, which this metadata cannot represent.
            if secondary.is_none()
                && definition.parent.is_none()
                && !ssa.connectors.is_empty()
                && matches!(
                    self.definition.organization,
                    DatabaseOrganization::Hdam | DatabaseOrganization::Dedb
                )
            {
                return Err(EngineProblem::Unsupported);
            }
            if ssa.connectors.contains(&ImsSsaBoolean::IndependentAnd) {
                if secondary.is_none() {
                    return Err(EngineProblem::Unsupported);
                }
                if self.independent_index(ssa, secondary).is_some() {
                    // Overlapping ranges, repeated equality groups and mixed
                    // independent sets need additional group/search semantics.
                    // Admit only disjoint equality groups; never flatten them.
                    let mut keys = BTreeSet::new();
                    if ssa
                        .connectors
                        .iter()
                        .any(|c| *c != ImsSsaBoolean::IndependentAnd)
                        || ssa.predicates.iter().any(|p| {
                            p.relation != ImsSsaRelation::Equal || !keys.insert(p.value.as_slice())
                        })
                    {
                        return Err(EngineProblem::Unsupported);
                    }
                }
            }
        }
        if count > self.limits.max_predicates {
            return Err(EngineProblem::LimitExceeded);
        }
        Ok(())
    }

    fn independent_index<'a>(
        &'a self,
        ssa: &ImsSsa,
        secondary: Option<&'a str>,
    ) -> Option<&'a SecondaryIndexDefinition> {
        if !ssa.connectors.contains(&ImsSsaBoolean::IndependentAnd) {
            return None;
        }
        let ImsSsaField::Named(name) = &ssa.predicates.first()?.field else {
            return None;
        };
        let fields = self.ssa_fields(secondary);
        let index = fields.selected_index_field(&ssa.segment, name)?;
        if !ssa
            .predicates
            .iter()
            .all(|p| matches!(&p.field, ImsSsaField::Named(name) if name == &index.name))
        {
            return None;
        }
        Some(index)
    }

    fn matches_ssa(&self, id: RecordId, ssa: &ImsSsa, indexed: Option<(&str, &[u8])>) -> bool {
        if let Some(key) = &ssa.concatenated_key {
            let Ok(path) = self.record_path(id) else {
                return false;
            };
            let mut actual = Vec::new();
            for id in path {
                let record = &self.records[&id];
                let Ok(s) = self.segment(&record.segment) else {
                    return false;
                };
                let Some(value) = s
                    .key_field
                    .as_deref()
                    .and_then(|name| navigation::field_value(s, name, &record.data))
                else {
                    return false;
                };
                actual.extend(value);
            }
            return actual == *key;
        }
        let record = &self.records[&id];
        let Ok(definition) = self.segment(&record.segment) else {
            return false;
        };
        if let Some(index) = self.independent_index(ssa, indexed.map(|(name, _)| name)) {
            return indexed.is_some_and(|(_, key)| ssa.predicates.iter().any(|p| p.value == key))
                && ssa.predicates.iter().all(|p| {
                    self.indexes[&index.name]
                        .get(&p.value)
                        .is_some_and(|sources| {
                            sources.iter().any(|source| {
                                self.index_target(&index.name, *source)
                                    .is_ok_and(|target| target == id)
                            })
                        })
                });
        }
        let evaluate = |p: &mainframe_env_host_api::ImsSsaPredicate| {
            let actual = match &p.field {
                ImsSsaField::Named(name)
                    if self
                        .ssa_fields(indexed.map(|(index, _)| index))
                        .selected_index_field(&record.segment, name)
                        .is_some() =>
                {
                    indexed.map(|(_, key)| key)
                }
                ImsSsaField::Named(name) => definition
                    .fields
                    .iter()
                    .find(|f| &f.name == name)
                    .and_then(|f| record.data.get(f.offset..f.offset + f.length)),
                ImsSsaField::Offset { position, length } => {
                    let start = usize::from(*position) - 1;
                    record.data.get(start..start + usize::from(*length))
                }
            };
            actual.is_some_and(|actual| match p.relation {
                ImsSsaRelation::Equal => actual == p.value,
                ImsSsaRelation::NotEqual => actual != p.value,
                ImsSsaRelation::LessThan => actual < p.value.as_slice(),
                ImsSsaRelation::LessOrEqual => actual <= p.value.as_slice(),
                ImsSsaRelation::GreaterThan => actual > p.value.as_slice(),
                ImsSsaRelation::GreaterOrEqual => actual >= p.value.as_slice(),
            })
        };
        // Each OR starts another AND set. Preserve the AST connector identities;
        // secondary # with a physical field has source-defined dependent behavior.
        let mut set = true;
        for (i, predicate) in ssa.predicates.iter().enumerate() {
            if i > 0 && ssa.connectors[i - 1] == ImsSsaBoolean::LogicalOr {
                if set {
                    return true;
                }
                set = true;
            }
            set &= evaluate(predicate);
        }
        set
    }
}
