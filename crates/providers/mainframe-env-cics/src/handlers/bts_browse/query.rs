//! Deterministic projections of versioned BTS lifecycle rows into cursor items.

use super::{BrowseItem, MAX_CURSOR_ITEMS};
use crate::service::handlers::bts_lifecycle::BtsProcess;
use mainframe_env_host_api::HostProblem;

pub fn process_snapshot(processes: &[BtsProcess]) -> Result<Vec<BrowseItem>, HostProblem> {
    if processes.len() > MAX_CURSOR_ITEMS {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut ordered = processes.iter().collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.name.cmp(&b.name));
    if ordered.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(HostProblem::InfrastructureFailure);
    }
    ordered
        .into_iter()
        .map(|process| {
            BrowseItem::new(&process.name, Some(&process.root_id), 0)?.with_epoch(process.epoch)
        })
        .collect()
}

pub fn activity_children_snapshot(
    process: &BtsProcess,
    parent_id: &str,
    uow: &str,
) -> Result<Vec<BrowseItem>, HostProblem> {
    process.validate()?;
    if !process.visible_to(uow)
        || !process.activities.get(parent_id).is_some_and(|activity| {
            activity
                .pending_uow
                .as_deref()
                .is_none_or(|owner| owner == uow)
        })
    {
        return Err(HostProblem::NotFound);
    }
    let mut children = process
        .activities
        .values()
        .filter(|activity| {
            activity.parent_id.as_deref() == Some(parent_id)
                && activity
                    .pending_uow
                    .as_deref()
                    .is_none_or(|owner| owner == uow)
        })
        .collect::<Vec<_>>();
    children.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    if children.len() > MAX_CURSOR_ITEMS {
        return Err(HostProblem::ResourceExhausted);
    }
    children
        .into_iter()
        .map(|activity| {
            BrowseItem::new(&activity.name, Some(&activity.id), 0)?.with_epoch(process.epoch)
        })
        .collect()
}

pub fn activity_flat_snapshot(
    process: &BtsProcess,
    uow: &str,
) -> Result<Vec<BrowseItem>, HostProblem> {
    process.validate()?;
    if !process.visible_to(uow) {
        return Err(HostProblem::NotFound);
    }
    let mut result = Vec::new();
    let mut stack = vec![(process.root_id.as_str(), 0u16)];
    while let Some((id, level)) = stack.pop() {
        let activity = process
            .activities
            .get(id)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if activity
            .pending_uow
            .as_deref()
            .is_some_and(|owner| owner != uow)
        {
            continue;
        }
        result.push(
            BrowseItem::new(&activity.name, Some(&activity.id), level)?
                .with_epoch(process.epoch)?,
        );
        if result.len() > MAX_CURSOR_ITEMS {
            return Err(HostProblem::ResourceExhausted);
        }
        let next_level = level.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
        let mut children = process
            .activities
            .values()
            .filter(|child| child.parent_id.as_deref() == Some(id))
            .collect::<Vec<_>>();
        children.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        for child in children.into_iter().rev() {
            stack.push((&child.id, next_level));
        }
    }
    Ok(result)
}
