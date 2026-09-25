//! Referential and queue invariants for the durable BTS activity record.

use super::{ActivityState, EventKind, MAX_EVENTS, MAX_QUEUE, MAX_REPLAYS, MAX_TIMERS, event_name};
use mainframe_env_host_api::HostProblem;
use std::collections::BTreeSet;

pub(super) fn validate(state: &ActivityState) -> Result<(), HostProblem> {
    if state.events.len() > MAX_EVENTS
        || state.timers.len() > MAX_TIMERS
        || state.reattach.len() > MAX_QUEUE
        || state.replays.len() > MAX_REPLAYS
    {
        return Err(HostProblem::ResourceExhausted);
    }
    for (name, event) in &state.events {
        if event_name(name).ok().as_deref() != Some(name)
            || name == "DFHINITIAL"
            || event
                .parent
                .as_deref()
                .is_some_and(|parent| event_name(parent).ok().as_deref() != Some(parent))
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if let Some(parent_name) = &event.parent {
            let parent = state
                .events
                .get(parent_name)
                .ok_or(HostProblem::InfrastructureFailure)?;
            let EventKind::Composite { children, .. } = &parent.kind else {
                return Err(HostProblem::InfrastructureFailure);
            };
            if !children.iter().any(|child| child == name)
                || matches!(event.kind, EventKind::Composite { .. })
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        match &event.kind {
            EventKind::Input => {}
            EventKind::Activity { child_id } => {
                if child_id.len() != 52 || !child_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            EventKind::Timer { timer } => {
                let record = state
                    .timers
                    .get(timer)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if record.event != *name || record.acknowledged {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            EventKind::Composite {
                all,
                children,
                fired_queue,
            } => {
                if event.parent.is_some()
                    || children.len() > MAX_EVENTS
                    || fired_queue.len() > MAX_QUEUE
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let mut unique = BTreeSet::new();
                for child_name in children {
                    let child = state
                        .events
                        .get(child_name)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    if !unique.insert(child_name)
                        || child.parent.as_deref() != Some(name)
                        || matches!(child.kind, EventKind::Composite { .. })
                        || *all && matches!(child.kind, EventKind::Input)
                    {
                        return Err(HostProblem::InfrastructureFailure);
                    }
                }
                let actual = if *all {
                    children.iter().all(|child| state.events[child].fired)
                } else {
                    children.iter().any(|child| state.events[child].fired)
                };
                if event.fired != actual {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let mut queued = BTreeSet::new();
                for child_name in fired_queue {
                    if !queued.insert(child_name)
                        || !unique.contains(child_name)
                        || !state.events[child_name].fired
                    {
                        return Err(HostProblem::InfrastructureFailure);
                    }
                }
            }
        }
    }
    for (name, timer) in &state.timers {
        if event_name(name).ok().as_deref() != Some(name)
            || event_name(&timer.event).ok().as_deref() != Some(timer.event.as_str())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let associated = state.events.get(&timer.event);
        if timer.acknowledged && associated.is_some()
            || !timer.acknowledged
                && !associated.is_some_and(|event| {
                    matches!(&event.kind, EventKind::Timer { timer: owner } if owner == name)
                })
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let mut queued = BTreeSet::new();
    for name in &state.reattach {
        if !queued.insert(name)
            || !state
                .events
                .get(name)
                .is_some_and(|event| event.fired && event.parent.is_none())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}
