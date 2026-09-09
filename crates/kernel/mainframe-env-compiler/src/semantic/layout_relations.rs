//! Cross-layout rules for executable COBOL storage metadata.

use super::{
    CobolLayout, DataCategory, DataSpec, SemanticProblem, StorageSection, find_after_owned,
    resolve_layout_name, resolve_nearby_layout,
};
use mainframe_env_ir::{
    COBOL_MAX_TABLE_KEY_BYTES, cobol_table_key_category_is_eligible,
    validate_cobol_condition_values, validate_cobol_level78_value,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct RenameRange<'a> {
    pub(super) start: &'a CobolLayout,
    pub(super) end: &'a CobolLayout,
    pub(super) through: Option<String>,
}

pub(super) fn resolve_rename_range<'a>(
    spec: &DataSpec,
    specs: &[DataSpec],
    layouts: &'a [Option<CobolLayout>],
) -> Result<RenameRange<'a>, SemanticProblem> {
    let start_name = find_after_owned(&spec.words, "RENAMES")
        .ok_or_else(|| SemanticProblem::InvalidDeclaration(spec.sentence.clone()))?;
    let start = resolve_nearby_layout(&start_name, spec, specs, layouts)?;
    let through =
        find_after_owned(&spec.words, "THRU").or_else(|| find_after_owned(&spec.words, "THROUGH"));
    let end = through
        .as_ref()
        .map(|name| resolve_nearby_layout(name, spec, specs, layouts))
        .transpose()?
        .unwrap_or(start);
    if through.is_some() && end.qualified_name == start.qualified_name
        || end.offset < start.offset
        || end.offset.saturating_add(end.length) < start.offset.saturating_add(start.length)
    {
        return Err(SemanticProblem::InvalidRename);
    }
    let position = |target: &CobolLayout| {
        layouts.iter().position(|layout| {
            layout
                .as_ref()
                .is_some_and(|layout| layout.qualified_name == target.qualified_name)
        })
    };
    let start_index = position(start).ok_or(SemanticProblem::InvalidRename)?;
    let end_index = position(end).ok_or(SemanticProblem::InvalidRename)?;
    if through.is_some()
        && (end_index <= start_index
            || layouts[start_index + 1..end_index].iter().any(|candidate| {
                candidate
                    .as_ref()
                    .is_some_and(|candidate| candidate.depending_on.is_some())
            }))
    {
        return Err(SemanticProblem::InvalidRename);
    }
    Ok(RenameRange {
        start,
        end,
        through,
    })
}

pub(super) fn validate_layout_relationships(
    specs: &[DataSpec],
    layouts: &[CobolLayout],
    cics_context: bool,
) -> Result<(), SemanticProblem> {
    let by_name = layouts
        .iter()
        .enumerate()
        .map(|(index, layout)| (layout.qualified_name.as_str(), (index, layout)))
        .collect::<BTreeMap<_, _>>();
    let alias_participants = layouts
        .iter()
        .filter(|layout| layout.category != DataCategory::Condition)
        .filter_map(|layout| {
            layout
                .alias_of
                .as_ref()
                .map(|target| [layout.qualified_name.as_str(), target.as_str()])
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    let mut odo_descendant_ancestors = BTreeSet::new();
    let mut odo_by_root = BTreeMap::<&str, Vec<usize>>::new();
    let mut occurs_by_ancestor = BTreeMap::<&str, Vec<(usize, &str)>>::new();
    for (index, layout) in layouts.iter().enumerate() {
        if layout.depending_on.is_some() {
            let root = layout
                .qualified_name
                .split('.')
                .next()
                .unwrap_or(layout.qualified_name.as_str());
            odo_by_root.entry(root).or_default().push(index);
            let mut ancestor = layout.parent.as_deref();
            while let Some(name) = ancestor {
                odo_descendant_ancestors.insert(name);
                ancestor = by_name
                    .get(name)
                    .and_then(|(_, parent)| parent.parent.as_deref());
            }
        }
        if layout.occurs_clause {
            let mut ancestor = layout.parent.as_deref();
            while let Some(name) = ancestor {
                occurs_by_ancestor
                    .entry(name)
                    .or_default()
                    .push((index, layout.qualified_name.as_str()));
                ancestor = by_name
                    .get(name)
                    .and_then(|(_, parent)| parent.parent.as_deref());
            }
        }
    }

    for (index, spec) in specs.iter().enumerate() {
        let layout = &layouts[index];
        if layout.dynamic {
            if layout.section == StorageSection::File && layout.parent.is_some() {
                return Err(SemanticProblem::InvalidUsage(
                    "FILE SECTION dynamic item must be level 01 or 77".into(),
                ));
            }
            let mut current = Some(layout);
            while let Some(candidate) = current {
                if candidate.occurs_clause
                    || alias_participants.contains(candidate.qualified_name.as_str())
                {
                    return Err(SemanticProblem::InvalidUsage(
                        "dynamic layout cannot participate in a table or alias hierarchy".into(),
                    ));
                }
                current = candidate
                    .parent
                    .as_deref()
                    .and_then(|parent| by_name.get(parent).map(|(_, layout)| *layout));
            }
        }

        if let Some(name) = &layout.depending_on
            && !(cics_context && name.eq_ignore_ascii_case("EIBCALEN"))
        {
            let target = resolve_layout_name(name, spec, specs, layouts)?;
            let (target_index, _) = by_name
                .get(target.qualified_name.as_str())
                .copied()
                .ok_or(SemanticProblem::InvalidOccurs)?;
            let mut ancestor = target.parent.as_deref();
            while let Some(name) = ancestor {
                let (_, parent) = by_name
                    .get(name)
                    .copied()
                    .ok_or(SemanticProblem::InvalidOccurs)?;
                if parent.occurs_clause {
                    return Err(SemanticProblem::InvalidOccurs);
                }
                ancestor = parent.parent.as_deref();
            }
            let root = target
                .qualified_name
                .split('.')
                .next()
                .unwrap_or(target.qualified_name.as_str());
            if !matches!(
                target.category,
                DataCategory::NumericDisplay | DataCategory::PackedDecimal | DataCategory::Binary
            ) || target.name == "FILLER"
                || target.scale != 0
                || target.dynamic
                || target.occurs_clause
                || target.typedef
                || !target.allocated
                || target
                    .qualified_name
                    .starts_with(&format!("{}.", spec.qualified))
                || odo_by_root.get(root).is_some_and(|entries| {
                    entries.partition_point(|entry| *entry < target_index) > 0
                })
            {
                return Err(SemanticProblem::InvalidOccurs);
            }
        }

        let mut key_extent = 0u64;
        for key in &layout.keys {
            let target = resolve_table_key_name(&key.name, spec, specs, layouts)?;
            let (target_index, _) = by_name
                .get(target.qualified_name.as_str())
                .copied()
                .ok_or(SemanticProblem::InvalidOccurs)?;
            if target.name == "FILLER"
                || !cobol_table_key_category_is_eligible(target.category.executable_name())
                || odo_descendant_ancestors.contains(target.qualified_name.as_str())
            {
                return Err(SemanticProblem::InvalidOccurs);
            }
            let subject = target.qualified_name == spec.qualified;
            if subject {
                if layout.keys.len() != 1 {
                    return Err(SemanticProblem::InvalidOccurs);
                }
            } else {
                if !target
                    .qualified_name
                    .starts_with(&format!("{}.", spec.qualified))
                    || target.occurs_clause
                    || target.dynamic
                    || target.unbounded
                {
                    return Err(SemanticProblem::InvalidOccurs);
                }
                let mut ancestor = target.parent.as_deref();
                while ancestor.is_some_and(|parent| parent != spec.qualified) {
                    let (_, parent) = by_name
                        .get(ancestor.ok_or(SemanticProblem::InvalidOccurs)?)
                        .copied()
                        .ok_or(SemanticProblem::InvalidOccurs)?;
                    if parent.occurs_clause {
                        return Err(SemanticProblem::InvalidOccurs);
                    }
                    ancestor = parent.parent.as_deref();
                }
                if ancestor.is_none() {
                    return Err(SemanticProblem::InvalidOccurs);
                }
                if let Some(entries) = occurs_by_ancestor.get(spec.qualified.as_str()) {
                    let before = entries.partition_point(|(entry, _)| *entry < target_index);
                    if entries[..before]
                        .iter()
                        .rev()
                        .any(|(_, table)| !target.qualified_name.starts_with(&format!("{table}.")))
                    {
                        return Err(SemanticProblem::InvalidOccurs);
                    }
                }
            }
            key_extent = key_extent
                .checked_add(target.element_length as u64)
                .filter(|extent| *extent <= COBOL_MAX_TABLE_KEY_BYTES)
                .ok_or(SemanticProblem::InvalidOccurs)?;
        }

        if layout.category == DataCategory::Condition {
            if layout.length != 0
                || layout.element_length != 0
                || layout.condition_values.is_empty()
            {
                return Err(SemanticProblem::InvalidDeclaration(layout.name.clone()));
            }
            let values = layout
                .condition_values
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            match (&layout.parent, &layout.alias_of) {
                (None, None) => validate_cobol_level78_value(&values)
                    .map_err(|_| SemanticProblem::InvalidDeclaration(layout.name.clone()))?,
                (Some(parent), Some(alias)) if parent.eq_ignore_ascii_case(alias) => {
                    let (target_index, target) = by_name
                        .get(alias.as_str())
                        .copied()
                        .ok_or_else(|| SemanticProblem::InvalidReference(alias.clone()))?;
                    if target_index >= index
                        || matches!(
                            target.category,
                            DataCategory::Condition | DataCategory::Rename
                        )
                    {
                        return Err(SemanticProblem::InvalidDeclaration(layout.name.clone()));
                    }
                    validate_cobol_condition_values(
                        target.category.executable_name(),
                        target.digits as u64,
                        target.scale as u64,
                        target.signed,
                        target.element_length as u64,
                        &values,
                    )
                    .map_err(|_| SemanticProblem::InvalidDeclaration(layout.name.clone()))?;
                }
                _ => return Err(SemanticProblem::InvalidDeclaration(layout.name.clone())),
            }
        } else if layout.category == DataCategory::Rename {
            let alias = layout
                .alias_of
                .as_deref()
                .ok_or(SemanticProblem::InvalidRename)?;
            let owner = layout
                .parent
                .as_deref()
                .ok_or(SemanticProblem::InvalidRename)?;
            let (target_index, target) = by_name
                .get(alias)
                .copied()
                .ok_or(SemanticProblem::InvalidRename)?;
            if target_index >= index
                || layout.offset != target.offset
                || layout.length < target.length
                || !target.qualified_name.starts_with(&format!("{owner}."))
            {
                return Err(SemanticProblem::InvalidRename);
            }
            let mut current = Some(target);
            while let Some(candidate) = current {
                if candidate.occurs_clause {
                    return Err(SemanticProblem::InvalidRename);
                }
                if candidate.parent.as_deref() == Some(owner) {
                    break;
                }
                current = candidate
                    .parent
                    .as_deref()
                    .and_then(|parent| by_name.get(parent).map(|(_, layout)| *layout));
                if current.is_none() {
                    return Err(SemanticProblem::InvalidRename);
                }
            }
        } else if let Some(alias) = &layout.alias_of {
            let (_, target) = by_name
                .get(alias.as_str())
                .copied()
                .ok_or_else(|| SemanticProblem::UnknownRedefines(alias.clone()))?;
            if target.parent != layout.parent || target.occurs_clause {
                return Err(SemanticProblem::InvalidRedefines(layout.name.clone()));
            }
            if target.external_name.is_some() && layout.length > target.length {
                return Err(SemanticProblem::InvalidRedefines(layout.name.clone()));
            }
        }
    }
    Ok(())
}

fn resolve_table_key_name<'a>(
    name: &str,
    spec: &DataSpec,
    specs: &[DataSpec],
    layouts: &'a [CobolLayout],
) -> Result<&'a CobolLayout, SemanticProblem> {
    if !name.contains('.') && name.split_whitespace().count() == 1 {
        let prefix = format!("{}.", spec.qualified);
        let candidates = layouts
            .iter()
            .filter(|layout| {
                layout.name.eq_ignore_ascii_case(name) && layout.qualified_name.starts_with(&prefix)
            })
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [target] => return Ok(*target),
            [] => {}
            _ => {
                return Err(SemanticProblem::InvalidReference(format!(
                    "ambiguous {name}"
                )));
            }
        }
    }
    resolve_layout_name(name, spec, specs, layouts)
}
