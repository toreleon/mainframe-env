//! CHECK process or activity using one validated durable lifecycle row.

use super::*;
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let (process, activity_id, parent_check) = match request.operation {
        CicsOperation::CheckAcqActivity => {
            let (_, process, id) = acquisition(service, run, false)?;
            (process, id, None)
        }
        CicsOperation::CheckAcqProcess => {
            let (_, process, id) = acquisition(service, run, true)?;
            (process, id, None)
        }
        CicsOperation::CheckActivity => {
            let context = active_context(service, run)?;
            let name = argument_name(request, "ACTIVITY", 16)?;
            let authority = BtsLifecycleStore::new(service.store.as_ref());
            let process = authority
                .load_process(&context.process_type, &context.process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let child = process
                .child(&context.activity_id, &name)
                .filter(|child| {
                    child
                        .pending_uow
                        .as_deref()
                        .is_none_or(|uow| uow == context.run_unit)
                })
                .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?;
            let id = child.id.clone();
            (process, id, Some((context, name)))
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    service.authorize(
        run,
        "BTSLIFE",
        &BtsLifecycleStore::saf_resource(&process.process_type, &process.name)?,
        AccessIntent::Read,
    )?;
    let activity = if let Some((context, name)) = parent_check {
        BtsLifecycleStore::new(service.store.as_ref()).checked_child_and_ack(&context, &name)?
    } else {
        process
            .activities
            .get(&activity_id)
            .ok_or(HostProblem::InfrastructureFailure)?
            .clone()
    };
    let mut outputs = BTreeMap::new();
    for (name, value) in [
        ("COMPSTATUS", completion(activity.completion)),
        ("MODE", mode(activity.mode)),
        (
            "SUSPSTATUS",
            if activity.suspended {
                "SUSPENDED"
            } else {
                "NOTSUSPENDED"
            },
        ),
    ] {
        if request.arguments.contains_key(name) {
            outputs.insert(
                name.into(),
                ("mainframe-env.cics.cvda@1", value.as_bytes().to_vec()),
            );
        }
    }
    for (name, value, width) in [
        ("ABCODE", activity.abcode.as_deref(), 4),
        ("ABPROGRAM", activity.abprogram.as_deref(), 8),
    ] {
        if request.arguments.contains_key(name) {
            let mut bytes = value.unwrap_or("").as_bytes().to_vec();
            bytes.resize(width, b' ');
            outputs.insert(name.into(), ("mainframe-env.cics.payload@1", bytes));
        }
    }
    response(service, run, outputs)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let selector = match request.operation {
        CicsOperation::CheckAcqActivity => Some("OPTION.ACQACTIVITY"),
        CicsOperation::CheckAcqProcess => Some("OPTION.ACQPROCESS"),
        CicsOperation::CheckActivity => None,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if selector.is_some_and(|name| !request.arguments.contains_key(name))
        || request.operation == CicsOperation::CheckActivity
            && !request.arguments.contains_key("ACTIVITY")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        if name.starts_with("OPTION.") {
            if !matches!(name.as_str(), "OPTION.NOHANDLE") && selector != Some(name.as_str())
                || value.schema() != "mainframe-env.cics.option@1"
                || !value.bytes().is_empty()
            {
                return Err(HostProblem::Malformed);
            }
        } else if name == "ACTIVITY" {
            if selector.is_some()
                || !matches!(
                    value.schema(),
                    "mainframe-env.cics.argument@1"
                        | "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                )
            {
                return Err(HostProblem::Malformed);
            }
        } else if !matches!(
            name.as_str(),
            "COMPSTATUS" | "MODE" | "SUSPSTATUS" | "ABCODE" | "ABPROGRAM" | "RESP" | "RESP2"
        ) || value.schema() != "mainframe-env.cics.argument@1"
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn completion(value: BtsCompletion) -> &'static str {
    match value {
        BtsCompletion::Incomplete => "INCOMPLETE",
        BtsCompletion::Normal => "NORMAL",
        BtsCompletion::Abend => "ABEND",
        BtsCompletion::Forced => "FORCED",
    }
}

fn mode(value: BtsMode) -> &'static str {
    match value {
        BtsMode::Initial => "INITIAL",
        BtsMode::Active => "ACTIVE",
        BtsMode::Dormant => "DORMANT",
        BtsMode::Cancelling => "CANCELLING",
        BtsMode::Complete => "COMPLETE",
    }
}
