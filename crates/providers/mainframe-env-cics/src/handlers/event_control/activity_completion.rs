//! Atomic event-pool edits supplied to the shared BTS lifecycle authority.

use super::{
    ActivityState, EventKind, EventRecord, MAX_EVENTS, activity_mutation, composite, event_error,
    event_name, fire_atomic, load_activity_from_store,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateStore};

pub(in crate::service) fn define(
    store: &dyn ProviderStateStore,
    parent_id: &str,
    event: &str,
    child_id: &str,
) -> Result<ProviderStateMutation, HostProblem> {
    let event = event_name(event).map_err(|_| event_error(6))?;
    let mut state = load_activity_from_store(store, parent_id)?;
    if state.events.contains_key(&event) {
        return Err(event_error(7));
    }
    if state.events.len() >= MAX_EVENTS {
        return Err(HostProblem::ResourceExhausted);
    }
    state.events.insert(
        event,
        EventRecord {
            kind: EventKind::Activity {
                child_id: child_id.into(),
            },
            fired: false,
            parent: None,
        },
    );
    activity_mutation(parent_id, &state)
}

pub(in crate::service) fn reset(
    store: &dyn ProviderStateStore,
    parent_id: &str,
    event: &str,
    child_id: &str,
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    let mut state = load_activity_from_store(store, parent_id)?;
    if state.version == 0 {
        return Ok(None);
    }
    let record = activity_record(&state, event, child_id)?;
    let composite_parent = record.parent.clone();
    state.events.get_mut(event).expect("validated event").fired = false;
    state.reattach.retain(|name| name != event);
    if let Some(parent) = composite_parent {
        let composite = state
            .events
            .get_mut(&parent)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let EventKind::Composite { fired_queue, .. } = &mut composite.kind else {
            return Err(HostProblem::InfrastructureFailure);
        };
        fired_queue.retain(|name| name != event);
        composite::reevaluate(&mut state, &parent)?;
    }
    Ok(Some(activity_mutation(parent_id, &state)?))
}

pub(in crate::service) fn delete(
    store: &dyn ProviderStateStore,
    parent_id: &str,
    event: &str,
    child_id: &str,
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    delete_many(
        store,
        parent_id,
        &[(event.to_string(), child_id.to_string())],
    )
}

pub(in crate::service) fn delete_many(
    store: &dyn ProviderStateStore,
    parent_id: &str,
    children: &[(String, String)],
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    let mut state = load_activity_from_store(store, parent_id)?;
    if state.version == 0 {
        return Ok(None);
    }
    for (event, child_id) in children {
        let record = activity_record(&state, event, child_id)?;
        if let Some(parent) = record.parent.clone() {
            let composite = state
                .events
                .get_mut(&parent)
                .ok_or(HostProblem::InfrastructureFailure)?;
            let EventKind::Composite {
                children,
                fired_queue,
                ..
            } = &mut composite.kind
            else {
                return Err(HostProblem::InfrastructureFailure);
            };
            children.retain(|name| name != event);
            fired_queue.retain(|name| name != event);
            state.events.remove(event);
            composite::reevaluate(&mut state, &parent)?;
        } else {
            state.events.remove(event);
        }
        state.reattach.retain(|name| name != event);
    }
    Ok(Some(activity_mutation(parent_id, &state)?))
}

pub(in crate::service) fn post_many(
    store: &dyn ProviderStateStore,
    parent_id: &str,
    children: &[(String, String)],
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    let mut state = load_activity_from_store(store, parent_id)?;
    if state.version == 0 {
        return Ok(None);
    }
    let mut changed = false;
    for (event, child_id) in children {
        activity_record(&state, event, child_id)?;
        changed |= fire_atomic(&mut state, event)?;
    }
    if changed {
        Ok(Some(activity_mutation(parent_id, &state)?))
    } else {
        Ok(None)
    }
}

pub(in crate::service) fn delete_pool(
    store: &dyn ProviderStateStore,
    activity_id: &str,
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    let state = load_activity_from_store(store, activity_id)?;
    if state.version == 0 {
        return Ok(None);
    }
    Ok(Some(ProviderStateMutation::Delete {
        namespace: super::ACTIVITY_NAMESPACE.into(),
        key: activity_id.into(),
        expected_version: state.version,
    }))
}

fn activity_record<'a>(
    state: &'a ActivityState,
    event: &str,
    child_id: &str,
) -> Result<&'a EventRecord, HostProblem> {
    let record = state
        .events
        .get(event)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if !matches!(&record.kind, EventKind::Activity { child_id: owner } if owner == child_id) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}
