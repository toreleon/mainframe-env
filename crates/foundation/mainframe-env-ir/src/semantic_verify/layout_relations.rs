//! Category-specific cross-layout relationships.

use super::LayoutIndex;
use crate::Operation;
use crate::cobol_layout::CobolLayoutAbi;

pub(super) fn validate_rename(
    definition: &Operation,
    layout: &CobolLayoutAbi<'_>,
    layouts: &LayoutIndex<'_>,
) -> Result<(), &'static str> {
    if layout.alias_of.is_empty() || layout.parent.is_empty() {
        return Err("COBOL RENAMES association is missing");
    }
    let Some([target]) = layouts.get(&definition.identity, layout.alias_of) else {
        return Err("COBOL RENAMES has no unique range start");
    };
    if target.id >= definition.id {
        return Err("COBOL RENAMES range start is not prior");
    }
    let target = layouts.abi(target)?;
    if matches!(target.category, "condition" | "rename")
        || layout.offset != target.offset
        || layout.length < target.length
        || !target
            .name
            .to_ascii_uppercase()
            .starts_with(&format!("{}.", layout.parent.to_ascii_uppercase()))
    {
        return Err("COBOL RENAMES range start is ineligible");
    }
    validate_endpoint_hierarchy(layouts, definition, layout, target, "start")?;
    if layout.rename_through.is_empty() {
        if layout.length != target.length {
            return Err("single-item COBOL RENAMES extent is not exact");
        }
        return Ok(());
    }
    let Some([end]) = layouts.get(&definition.identity, layout.rename_through) else {
        return Err("COBOL RENAMES has no unique range end");
    };
    if end.id <= target.id || end.id >= definition.id {
        return Err("COBOL RENAMES range endpoints are not ordered and distinct");
    }
    let end = layouts.abi(end)?;
    if matches!(end.category, "condition" | "rename")
        || !end
            .name
            .to_ascii_uppercase()
            .starts_with(&format!("{}.", layout.parent.to_ascii_uppercase()))
    {
        return Err("COBOL RENAMES range end is ineligible");
    }
    validate_endpoint_hierarchy(layouts, definition, layout, end, "end")?;
    let expected = end
        .offset
        .checked_add(end.length)
        .and_then(|end| end.checked_sub(layout.offset))
        .ok_or("COBOL RENAMES range extent overflows")?;
    if layout.length != expected
        || has_odo_in_rename_range(layouts, &definition.identity, layout.parent, &target, &end)?
    {
        return Err("COBOL RENAMES range metadata is inconsistent");
    }
    Ok(())
}

fn has_odo_in_rename_range(
    layouts: &LayoutIndex<'_>,
    identity: &crate::OperationIdentity,
    owner: &str,
    start: &CobolLayoutAbi<'_>,
    end: &CobolLayoutAbi<'_>,
) -> Result<bool, &'static str> {
    for definition in layouts
        .definitions
        .get(identity)
        .into_iter()
        .flat_map(|definitions| definitions.values())
        .flatten()
    {
        let candidate = layouts.abi(definition)?;
        let ordered_between = candidate.id > start.id && candidate.id < end.id;
        let extent_between = candidate.id != start.id
            && candidate.id != end.id
            && candidate
                .name
                .to_ascii_uppercase()
                .starts_with(&format!("{}.", owner.to_ascii_uppercase()))
            && candidate.offset >= start.offset
            && candidate.offset <= end.offset;
        if !candidate.depending_on.is_empty() && (ordered_between || extent_between) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_endpoint_hierarchy<'a>(
    layouts: &LayoutIndex<'a>,
    definition: &Operation,
    layout: &CobolLayoutAbi<'_>,
    mut current: CobolLayoutAbi<'a>,
    endpoint: &str,
) -> Result<(), &'static str> {
    loop {
        if current.occurs_clause {
            return Err(if endpoint == "start" {
                "COBOL RENAMES range start is subordinate to a table"
            } else {
                "COBOL RENAMES range end is subordinate to a table"
            });
        }
        if current.parent.eq_ignore_ascii_case(layout.parent) {
            return Ok(());
        }
        let Some([parent]) = layouts.get(&definition.identity, current.parent) else {
            return Err(if endpoint == "start" {
                "COBOL RENAMES range start has a broken hierarchy"
            } else {
                "COBOL RENAMES range end has a broken hierarchy"
            });
        };
        current = layouts.abi(parent)?;
    }
}
