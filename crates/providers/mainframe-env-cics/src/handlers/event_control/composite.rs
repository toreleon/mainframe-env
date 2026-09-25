use super::{
    ActivityState, EventKind, EventOutput, EventRecord, MAX_EVENTS, MAX_QUEUE, context,
    event_error, event_name, mutate_activity,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsRequest, CicsResponse, HostProblem};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn define(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let event = input_name(request, "EVENT", 6)?;
    let all = request.arguments.contains_key("OPTION.AND");
    let any = request.arguments.contains_key("OPTION.OR");
    if all == any {
        return Err(HostProblem::Malformed);
    }
    let children = (1..=8)
        .filter_map(|index| {
            let clause = format!("SUBEVENT{index}");
            request
                .arguments
                .contains_key(&clause)
                .then_some((index, clause))
        })
        .map(|(index, clause)| input_name(request, &clause, 30 + index).map(|name| (index, name)))
        .collect::<Result<Vec<_>, _>>()?;
    let context = context(service, run)?;
    service.authorize(
        run,
        "BTSEVENT",
        &format!("CICS.BTS.{}.{}", context.activity, event),
        AccessIntent::Update,
    )?;
    for (_, child) in &children {
        service.authorize(
            run,
            "BTSEVENT",
            &format!("CICS.BTS.{}.{}", context.activity, child),
            AccessIntent::Read,
        )?;
    }
    mutate_activity(service, run, request, &context.activity, |state| {
        define_state(state, &event, all, &children)?;
        Ok(BTreeMap::<String, EventOutput>::new())
    })
}

pub(super) fn add(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    membership(service, run, request, true)
}

pub(super) fn remove(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    membership(service, run, request, false)
}

fn membership(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    adding: bool,
) -> Result<CicsResponse, HostProblem> {
    for (name, value) in &request.arguments {
        if matches!(name.as_str(), "EVENT" | "SUBEVENT") {
            if !matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ) {
                return Err(HostProblem::Malformed);
            }
        } else if name == "OPTION.NOHANDLE" {
            if value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty() {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(name.as_str(), "RESP" | "RESP2") {
            if value.schema() != "mainframe-env.cics.argument@1" {
                return Err(HostProblem::Malformed);
            }
        } else {
            return Err(HostProblem::Malformed);
        }
    }
    let event = membership_name(request, "EVENT", 4)?;
    let child = membership_name(request, "SUBEVENT", 5)?;
    let context = context(service, run)?;
    service.authorize(
        run,
        "BTSEVENT",
        &format!("CICS.BTS.{}.{}", context.activity, event),
        AccessIntent::Update,
    )?;
    service.authorize(
        run,
        "BTSEVENT",
        &format!("CICS.BTS.{}.{}", context.activity, child),
        AccessIntent::Read,
    )?;
    mutate_activity(service, run, request, &context.activity, |state| {
        if adding {
            add_state(state, &event, &child)?;
        } else {
            remove_state(state, &event, &child)?;
        }
        Ok(BTreeMap::new())
    })
}

fn membership_name(
    request: &CicsRequest,
    clause: &str,
    response2: i32,
) -> Result<String, HostProblem> {
    let raw = request
        .arguments
        .get(clause)
        .ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(raw.bytes()).map_err(|_| event_error(response2))?;
    event_name(text).map_err(|_| event_error(response2))
}

fn add_state(state: &mut ActivityState, event: &str, child: &str) -> Result<(), HostProblem> {
    let Some(parent) = state.events.get(event) else {
        return Err(event_error(4));
    };
    let (all, children) = match &parent.kind {
        EventKind::Composite { all, children, .. } => (*all, children),
        _ => return Err(invalid_membership(2)),
    };
    let Some(atomic) = state.events.get(child) else {
        return Err(event_error(5));
    };
    if child == event
        || child == "DFHINITIAL"
        || atomic.parent.is_some()
        || children.iter().any(|name| name == child)
        || matches!(atomic.kind, EventKind::Composite { .. })
        || all && matches!(atomic.kind, EventKind::Input)
    {
        return Err(invalid_membership(3));
    }
    if children.len() == MAX_EVENTS {
        return Err(HostProblem::ResourceExhausted);
    }
    let fired = atomic.fired;
    let parent = state.events.get_mut(event).expect("validated composite");
    let EventKind::Composite {
        children,
        fired_queue,
        ..
    } = &mut parent.kind
    else {
        unreachable!("validated composite")
    };
    if fired {
        if fired_queue.len() == MAX_QUEUE {
            return Err(HostProblem::ResourceExhausted);
        }
        fired_queue.push_back(child.into());
    }
    children.push(child.into());
    state.events.get_mut(child).expect("validated child").parent = Some(event.into());
    reevaluate(state, event)
}

fn remove_state(state: &mut ActivityState, event: &str, child: &str) -> Result<(), HostProblem> {
    let Some(parent) = state.events.get(event) else {
        return Err(event_error(4));
    };
    let EventKind::Composite { children, .. } = &parent.kind else {
        return Err(invalid_membership(2));
    };
    let Some(atomic) = state.events.get(child) else {
        return Err(event_error(5));
    };
    if atomic.parent.as_deref() != Some(event) || !children.iter().any(|name| name == child) {
        return Err(invalid_membership(3));
    }
    let parent = state.events.get_mut(event).expect("validated composite");
    let EventKind::Composite {
        children,
        fired_queue,
        ..
    } = &mut parent.kind
    else {
        unreachable!("validated composite")
    };
    children.retain(|name| name != child);
    fired_queue.retain(|name| name != child);
    state.events.get_mut(child).expect("validated child").parent = None;
    reevaluate(state, event)
}

pub(super) fn reevaluate(state: &mut ActivityState, event: &str) -> Result<(), HostProblem> {
    let record = state.events.get(event).ok_or_else(|| event_error(4))?;
    let EventKind::Composite { all, children, .. } = &record.kind else {
        return Err(invalid_membership(2));
    };
    let fired = if *all {
        children
            .iter()
            .all(|name| state.events.get(name).is_some_and(|child| child.fired))
    } else {
        children
            .iter()
            .any(|name| state.events.get(name).is_some_and(|child| child.fired))
    };
    let was_fired = record.fired;
    if fired && !was_fired && state.reattach.len() == MAX_QUEUE {
        return Err(HostProblem::ResourceExhausted);
    }
    state
        .events
        .get_mut(event)
        .expect("validated composite")
        .fired = fired;
    if fired && !was_fired {
        state.reattach.push_back(event.into());
    } else if !fired && was_fired {
        state.reattach.retain(|name| name != event);
    }
    Ok(())
}

fn invalid_membership(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    for (name, value) in &request.arguments {
        let numbered = name
            .strip_prefix("SUBEVENT")
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=8).contains(&number));
        if numbered || name == "EVENT" {
            if !matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ) {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(
            name.as_str(),
            "OPTION.AND" | "OPTION.OR" | "OPTION.NOHANDLE"
        ) {
            if value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty() {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(name.as_str(), "RESP" | "RESP2") {
            if value.schema() != "mainframe-env.cics.argument@1" {
                return Err(HostProblem::Malformed);
            }
        } else {
            return Err(HostProblem::Malformed);
        }
    }
    if !request.arguments.contains_key("EVENT") {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn input_name(
    request: &CicsRequest,
    clause: &str,
    invalid_response2: i32,
) -> Result<String, HostProblem> {
    let bytes = request.arguments[clause].bytes();
    let name = std::str::from_utf8(bytes).map_err(|_| invalid_name(clause, invalid_response2))?;
    event_name(name).map_err(|_| invalid_name(clause, invalid_response2))
}

fn invalid_name(clause: &str, response2: i32) -> HostProblem {
    if clause == "EVENT" {
        event_error(response2)
    } else {
        HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2,
        }
    }
}

fn define_state(
    state: &mut ActivityState,
    event: &str,
    all: bool,
    children: &[(i32, String)],
) -> Result<(), HostProblem> {
    if event == "DFHINITIAL" || state.events.contains_key(event) {
        return Err(event_error(7));
    }
    if state.events.len() == MAX_EVENTS {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut names = BTreeSet::new();
    for (index, child) in children {
        if child == "DFHINITIAL" || child == event || !names.insert(child) {
            return Err(invalid_name("SUBEVENT", 30 + index));
        }
        let Some(record) = state.events.get(child) else {
            return Err(event_error(20 + index));
        };
        if record.parent.is_some()
            || matches!(record.kind, EventKind::Composite { .. })
            || all && matches!(record.kind, EventKind::Input)
        {
            return Err(invalid_name("SUBEVENT", 30 + index));
        }
    }
    let fired_queue = children
        .iter()
        .filter(|(_, name)| state.events.get(name).is_some_and(|record| record.fired))
        .map(|(_, name)| name.clone())
        .collect::<VecDeque<_>>();
    let fired = if all {
        children.iter().all(|(_, name)| state.events[name].fired)
    } else {
        children.iter().any(|(_, name)| state.events[name].fired)
    };
    if fired && state.reattach.len() == MAX_QUEUE {
        return Err(HostProblem::ResourceExhausted);
    }
    for (_, child) in children {
        state.events.get_mut(child).expect("validated child").parent = Some(event.into());
    }
    state.events.insert(
        event.into(),
        EventRecord {
            kind: EventKind::Composite {
                all,
                children: children.iter().map(|(_, child)| child.clone()).collect(),
                fired_queue,
            },
            fired,
            parent: None,
        },
    );
    if fired {
        state.reattach.push_back(event.into());
    }
    Ok(())
}
