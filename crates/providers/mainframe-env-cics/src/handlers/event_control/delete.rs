use super::{EventKind, context, event_error, event_name, mutate_activity};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsRequest, CicsResponse, HostProblem};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    for (name, value) in &request.arguments {
        if name == "EVENT" {
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
    let raw = request
        .arguments
        .get("EVENT")
        .ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(raw.bytes()).map_err(|_| event_error(4))?;
    let name = event_name(text).map_err(|_| event_error(4))?;
    let context = context(service, run)?;
    service.authorize(
        run,
        "BTSEVENT",
        &format!("CICS.BTS.{}.{}", context.activity, name),
        AccessIntent::Update,
    )?;
    mutate_activity(service, run, request, &context.activity, |state| {
        if name == "DFHINITIAL" {
            return Err(invalid_kind());
        }
        let Some(record) = state.events.get(&name).cloned() else {
            return Err(event_error(4));
        };
        if matches!(
            record.kind,
            EventKind::Timer { .. } | EventKind::Activity { .. }
        ) {
            return Err(invalid_kind());
        }
        if let Some(parent_name) = record.parent {
            let Some(parent) = state.events.get_mut(&parent_name) else {
                return Err(HostProblem::InfrastructureFailure);
            };
            let EventKind::Composite {
                children,
                fired_queue,
                ..
            } = &mut parent.kind
            else {
                return Err(HostProblem::InfrastructureFailure);
            };
            children.retain(|child| child != &name);
            fired_queue.retain(|child| child != &name);
            state.events.remove(&name);
            super::composite::reevaluate(state, &parent_name)?;
        } else {
            if let EventKind::Composite { children, .. } = record.kind {
                for child in children {
                    state
                        .events
                        .get_mut(&child)
                        .ok_or(HostProblem::InfrastructureFailure)?
                        .parent = None;
                }
            }
            state.events.remove(&name);
        }
        state.reattach.retain(|event| event != &name);
        Ok(BTreeMap::new())
    })
}

fn invalid_kind() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 2,
    }
}
