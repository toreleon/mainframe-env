//! Metadata-resolved SSA selection on the existing navigation authority.
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

impl DatabaseEngine {
    pub(crate) fn read_ssas(
        &self,
        position: &mut PcbPosition,
        request: &ReadRequest,
        ssas: &[ImsSsa],
        visible: impl Fn(&str) -> bool,
    ) -> Result<RecordView, EngineProblem> {
        self.validate_ssas(request, ssas)?;
        if request.kind == ReadKind::NextInParent {
            let parent = position.parentage.ok_or(EngineProblem::ParentageRequired)?;
            let path = self.record_path(parent)?;
            for ssa in ssas {
                if let Some(id) = path
                    .iter()
                    .find(|id| self.records[id].segment == ssa.segment)
                    && !self.matches_ssa(*id, ssa)
                {
                    return Err(EngineProblem::PathMismatch);
                }
            }
        }
        let view = self.read_matching(position, request, |id| {
            visible(&self.records[&id].segment)
                && self.matches(id, request.target.as_deref(), &request.path)
                && self.record_path(id).is_ok_and(|path| {
                    ssas.iter().all(|ssa| {
                        path.iter()
                            .find(|id| self.records[id].segment == ssa.segment)
                            .is_some_and(|id| self.matches_ssa(*id, ssa))
                    })
                })
        })?;
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
    ) -> Result<(), EngineProblem> {
        self.validate_read(request)?;
        if request.hold && self.definition.organization == DatabaseOrganization::Gsam {
            return Err(EngineProblem::Unsupported);
        }
        let mut count = 0usize;
        for ssa in ssas {
            let definition = self.segment(&ssa.segment)?;
            count += ssa.predicates.len();
            for predicate in &ssa.predicates {
                let (offset, length) = match &predicate.field {
                    ImsSsaField::Named(name) => {
                        let f = definition
                            .fields
                            .iter()
                            .find(|f| &f.name == name)
                            .ok_or(EngineProblem::InvalidRequest)?;
                        (f.offset, f.length)
                    }
                    ImsSsaField::Offset { position, length } => {
                        (usize::from(*position) - 1, usize::from(*length))
                    }
                };
                if offset
                    .checked_add(length)
                    .is_none_or(|end| end > definition.max_length)
                    || length != predicate.value.len()
                {
                    return Err(EngineProblem::InvalidRequest);
                }
            }
            // The pinned scope names the connectors but does not freeze mixed-group
            // precedence. Admit uniform conjunction/disjunction, reject that class.
            let has_or = ssa.connectors.contains(&ImsSsaBoolean::LogicalOr);
            if has_or
                && ssa
                    .connectors
                    .iter()
                    .any(|c| *c != ImsSsaBoolean::LogicalOr)
            {
                return Err(EngineProblem::Unsupported);
            }
        }
        if count > self.limits.max_predicates {
            return Err(EngineProblem::LimitExceeded);
        }
        Ok(())
    }

    fn matches_ssa(&self, id: RecordId, ssa: &ImsSsa) -> bool {
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
        let evaluate = |p: &mainframe_env_host_api::ImsSsaPredicate| {
            let actual = match &p.field {
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
        if ssa.connectors.contains(&ImsSsaBoolean::LogicalOr) {
            ssa.predicates.iter().any(evaluate)
        } else {
            ssa.predicates.iter().all(evaluate)
        }
    }
}
