//! Destructive BTS reattachment/subevent retrieval and fire-status query.

use super::{
    EventKind, EventOutput, context, event_error, event_name, mutate_activity, outside_activity,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let context = context(service, run)?;
    let event = if request.operation == CicsOperation::RetrieveReattachEvent {
        None
    } else {
        let raw = request
            .arguments
            .get("EVENT")
            .ok_or(HostProblem::Malformed)?;
        let name = std::str::from_utf8(raw.bytes()).map_err(|_| event_error(4))?;
        Some(event_name(name).map_err(|_| event_error(4))?)
    };
    let now = super::timer::clock_millis(service, run)?;
    service.authorize(
        run,
        "BTSEVENT",
        &format!(
            "CICS.BTS.{}.{}",
            context.activity,
            event.as_deref().unwrap_or("REATTACH")
        ),
        if request.operation == CicsOperation::TestEvent {
            AccessIntent::Read
        } else {
            AccessIntent::Update
        },
    )?;
    mutate_activity(service, run, request, &context.activity, |state| {
        super::timer::refresh_due(state, now)?;
        let mut outputs = BTreeMap::new();
        match request.operation {
            CicsOperation::TestEvent => {
                let name = event.as_deref().expect("validated event input");
                let fired = state.events.get(name).ok_or_else(|| event_error(4))?.fired;
                outputs.insert(
                    "FIRESTATUS".into(),
                    cvda(if fired { "FIRED" } else { "NOTFIRED" }),
                );
            }
            CicsOperation::RetrieveReattachEvent => {
                let name = state.reattach.pop_front().ok_or_else(|| end(8))?;
                let record = state
                    .events
                    .get_mut(&name)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let kind = event_type(&record.kind);
                if !matches!(record.kind, EventKind::Composite { .. }) {
                    record.fired = false;
                }
                outputs.insert("EVENT".into(), padded_name(&name));
                outputs.insert("EVENTTYPE".into(), cvda(kind));
            }
            CicsOperation::RetrieveSubevent => {
                let name = event.as_deref().expect("validated composite input");
                let composite = state.events.get_mut(name).ok_or_else(|| event_error(4))?;
                let EventKind::Composite {
                    children,
                    fired_queue,
                    ..
                } = &mut composite.kind
                else {
                    return Err(invalid_composite());
                };
                if children.is_empty() {
                    return Err(end(10));
                }
                let child = fired_queue.pop_front().ok_or_else(|| end(9))?;
                let record = state
                    .events
                    .get_mut(&child)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if record.parent.as_deref() != Some(name) || !record.fired {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let kind = event_type(&record.kind);
                record.fired = false;
                super::composite::reevaluate(state, name)?;
                outputs.insert("SUBEVENT".into(), padded_name(&child));
                outputs.insert("EVENTTYPE".into(), cvda(kind));
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        }
        Ok(outputs)
    })
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed: &[&str] = match request.operation {
        CicsOperation::TestEvent => &["EVENT", "FIRESTATUS", "RESP", "RESP2", "OPTION.NOHANDLE"],
        CicsOperation::RetrieveReattachEvent => {
            &["EVENT", "EVENTTYPE", "RESP", "RESP2", "OPTION.NOHANDLE"]
        }
        CicsOperation::RetrieveSubevent => &[
            "EVENT",
            "SUBEVENT",
            "EVENTTYPE",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    for (name, value) in &request.arguments {
        if !allowed.contains(&name.as_str()) {
            return Err(HostProblem::Malformed);
        }
        let valid = match name.as_str() {
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "EVENT" if request.operation != CicsOperation::RetrieveReattachEvent => matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ),
            _ => value.schema() == "mainframe-env.cics.argument@1",
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    let required: &[&str] = match request.operation {
        CicsOperation::TestEvent => &["EVENT", "FIRESTATUS"],
        CicsOperation::RetrieveReattachEvent => &["EVENT", "EVENTTYPE"],
        CicsOperation::RetrieveSubevent => &["EVENT", "SUBEVENT", "EVENTTYPE"],
        _ => return Err(outside_activity()),
    };
    if !required
        .iter()
        .all(|name| request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn event_type(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::Input => "INPUT",
        EventKind::Activity { .. } => "ACTIVITY",
        EventKind::Composite { .. } => "COMPOSITE",
        EventKind::Timer { .. } => "TIMER",
    }
}

fn cvda(name: &str) -> EventOutput {
    EventOutput {
        schema: "mainframe-env.cics.cvda@1".into(),
        bytes: name.as_bytes().to_vec(),
    }
}

fn padded_name(name: &str) -> EventOutput {
    EventOutput {
        schema: "mainframe-env.cics.payload@1".into(),
        bytes: format!("{name:<16}").into_bytes(),
    }
}

fn end(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "END".into(),
        response: 83,
        response2,
    }
}

fn invalid_composite() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 2,
    }
}
