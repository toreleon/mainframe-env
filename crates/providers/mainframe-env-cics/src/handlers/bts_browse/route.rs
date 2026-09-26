//! Selected BTS browse command routes.

use super::super::bts_lifecycle::{BtsActivity, BtsLifecycleStore};
use super::*;
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostRequest,
    canonical_request_digest,
};

type Outputs = Vec<(&'static str, &'static str, Vec<u8>)>;
mod container;
mod event_timer;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if let Some(clock) = &service.replay_clock
        && clock.now_tick()? > run.invocation.deadline_tick
    {
        return Err(HostProblem::TimedOut);
    }
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let cursors = BtsBrowseStore::new(service.store.as_ref());
    let owner = super::owner(run)?;
    if request.operation.is_mutating() {
        let mutation = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?;
        let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
            .map_err(|_| HostProblem::ResourceExhausted)?;
        if let Some((outcome, scope)) =
            cursors.replay(&owner, mutation.idempotency_key.as_str(), digest)?
        {
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            let outputs = replay_outputs(request, outcome)?;
            return response(service, run, outputs);
        }
    }
    let outputs = match request.operation {
        operation if container::is_operation(operation) => {
            container::invoke(service, run, request, &authority, &cursors, &owner)?
        }
        operation if event_timer::is_operation(operation) => {
            event_timer::invoke(service, run, request, &authority, &cursors, &owner)?
        }
        CicsOperation::BtsStartBrowseProcess => {
            let process_type = text_arg(request, "PROCESSTYPE", 8)?;
            let definition = authority
                .load_process_type(&process_type)?
                .filter(|definition| definition.enabled)
                .ok_or_else(|| condition("PROCESSERR", 108, 4))?;
            service.authorize(
                run,
                "BTSREPO",
                &definition.repository_resource,
                AccessIntent::Read,
            )?;
            let processes = authority.list_processes(&process_type, owner.run_unit.as_str())?;
            if processes.is_empty() {
                return Err(condition("PROCESSERR", 108, 1));
            }
            let scope = BrowseScope::new(
                BrowseKind::Process,
                "BTSREPO",
                &definition.repository_resource,
                1,
            )?
            .with_process_type(&process_type)?;
            let items = process_snapshot(&processes)?;
            if items.iter().any(|item| item.name.len() > 36) {
                return Err(HostProblem::Unsupported);
            }
            let token = start(&cursors, &owner, request, scope, items)?;
            vec![(
                "BROWSETOKEN",
                "mainframe-env.cics.decimal@1",
                token.to_string().into_bytes(),
            )]
        }
        CicsOperation::BtsStartBrowseActivity => {
            let (process, items) = if request.arguments.contains_key("PROCESS") {
                let process_type = text_arg(request, "PROCESSTYPE", 8)?;
                let process_name = text_arg(request, "PROCESS", 36)?;
                let process = authority
                    .load_process(&process_type, &process_name)?
                    .filter(|process| process.visible_to(owner.run_unit.as_str()))
                    .ok_or_else(|| condition("PROCESSERR", 108, 3))?;
                let items = activity_flat_snapshot(&process, owner.run_unit.as_str())?;
                (process, items)
            } else {
                let id = if request.arguments.contains_key("ACTIVITYID") {
                    text_arg(request, "ACTIVITYID", 52)?
                } else {
                    authority
                        .active_context(&owner.run_unit, &owner.execution, &owner.principal)?
                        .ok_or_else(|| condition("ACTIVITYERR", 109, 2))?
                        .activity_id
                };
                let index = authority
                    .load_activity_index(&id)?
                    .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
                let process = authority
                    .load_process(&index.process_type, &index.process_name)?
                    .filter(|process| process.visible_to(owner.run_unit.as_str()))
                    .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
                let items = activity_children_snapshot(&process, &id, owner.run_unit.as_str())?;
                (process, items)
            };
            let resource = BtsLifecycleStore::saf_resource(&process.process_type, &process.name)?;
            service.authorize(run, "BTSLIFE", &resource, AccessIntent::Read)?;
            // GETNEXT ACTIVITY has a mandatory 16-character receiver.
            if items.iter().any(|item| item.name.len() > 16) {
                return Err(HostProblem::Unsupported);
            }
            let scope =
                BrowseScope::new(BrowseKind::Activity, "BTSLIFE", &resource, process.epoch)?
                    .with_process(&process.process_type, &process.name)?;
            let token = start(&cursors, &owner, request, scope, items)?;
            vec![(
                "BROWSETOKEN",
                "mainframe-env.cics.decimal@1",
                token.to_string().into_bytes(),
            )]
        }
        CicsOperation::BtsGetNextProcess | CicsOperation::BtsGetNextActivity => {
            let kind = if request.operation == CicsOperation::BtsGetNextProcess {
                BrowseKind::Process
            } else {
                BrowseKind::Activity
            };
            let token = token_arg(request)?;
            let scope = cursors.scope(&owner, token, kind)?;
            let item = cursors.peek(&owner, token, kind)?;
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            let (live_epoch, mut values) = if kind == BrowseKind::Process {
                let process_type = scope
                    .process_type
                    .as_deref()
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let process = authority
                    .load_process(process_type, &item.name)?
                    .filter(|process| process.visible_to(owner.run_unit.as_str()))
                    .ok_or_else(|| condition("PROCESSERR", 108, 3))?;
                let mut values = vec![(
                    "PROCESS",
                    "mainframe-env.cics.payload@1",
                    padded(&item.name, 36)?,
                )];
                if has(request, "ACTIVITYID") {
                    values.push((
                        "ACTIVITYID",
                        "mainframe-env.cics.payload@1",
                        padded(&process.root_id, 52)?,
                    ));
                }
                (process.epoch, values)
            } else {
                let process_type = scope
                    .process_type
                    .as_deref()
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let process_name = scope
                    .process_name
                    .as_deref()
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let process = authority
                    .load_process(process_type, process_name)?
                    .filter(|process| process.visible_to(owner.run_unit.as_str()))
                    .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
                if item.name.len() > 16 {
                    return Err(HostProblem::Unsupported);
                }
                let mut values = vec![(
                    "ACTIVITY",
                    "mainframe-env.cics.payload@1",
                    padded(&item.name, 16)?,
                )];
                if has(request, "ACTIVITYID") {
                    values.push((
                        "ACTIVITYID",
                        "mainframe-env.cics.payload@1",
                        padded(
                            item.activity_id
                                .as_deref()
                                .ok_or(HostProblem::InfrastructureFailure)?,
                            52,
                        )?,
                    ));
                }
                if has(request, "LEVEL") {
                    values.push((
                        "LEVEL",
                        "mainframe-env.cics.decimal@1",
                        item.level.to_string().into_bytes(),
                    ));
                }
                (process.epoch, values)
            };
            let outcome = effect(
                &cursors,
                &owner,
                request,
                BrowseEffect::Next {
                    token,
                    kind,
                    live_epoch,
                    expected: item,
                },
            )?;
            if !matches!(outcome, BrowseOutcome::Item(_)) {
                return Err(HostProblem::InfrastructureFailure);
            }
            std::mem::take(&mut values)
        }
        CicsOperation::BtsEndBrowseProcess | CicsOperation::BtsEndBrowseActivity => {
            let kind = if request.operation == CicsOperation::BtsEndBrowseProcess {
                BrowseKind::Process
            } else {
                BrowseKind::Activity
            };
            let token = token_arg(request)?;
            let scope = cursors.scope(&owner, token, kind)?;
            service.authorize(
                run,
                &scope.resource_class,
                &scope.resource_name,
                AccessIntent::Read,
            )?;
            let outcome = effect(&cursors, &owner, request, BrowseEffect::End { token, kind })?;
            if outcome != BrowseOutcome::Ended {
                return Err(HostProblem::InfrastructureFailure);
            }
            Vec::new()
        }
        CicsOperation::BtsInquireProcess => {
            let process_type = text_arg(request, "PROCESSTYPE", 8)?;
            let process_name = text_arg(request, "PROCESS", 36)?;
            let definition = authority
                .load_process_type(&process_type)?
                .filter(|definition| definition.enabled)
                .ok_or_else(|| condition("PROCESSERR", 108, 4))?;
            service.authorize(
                run,
                "BTSREPO",
                &definition.repository_resource,
                AccessIntent::Read,
            )?;
            let process = authority
                .load_process(&process_type, &process_name)?
                .filter(|process| process.visible_to(owner.run_unit.as_str()))
                .ok_or_else(|| condition("PROCESSERR", 108, 3))?;
            if has(request, "ACTIVITYID") {
                vec![(
                    "ACTIVITYID",
                    "mainframe-env.cics.payload@1",
                    padded(&process.root_id, 52)?,
                )]
            } else {
                Vec::new()
            }
        }
        CicsOperation::BtsInquireActivity => {
            let id = text_arg(request, "ACTIVITYID", 52)?;
            let index = authority
                .load_activity_index(&id)?
                .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
            let process = authority
                .load_process(&index.process_type, &index.process_name)?
                .filter(|process| process.visible_to(owner.run_unit.as_str()))
                .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
            let resource = BtsLifecycleStore::saf_resource(&process.process_type, &process.name)?;
            service.authorize(run, "BTSLIFE", &resource, AccessIntent::Read)?;
            let activity = process
                .activities
                .get(&id)
                .ok_or_else(|| condition("ACTIVITYERR", 109, 1))?;
            inquiry_outputs(request, &process, activity)?
        }
        _ => return Err(HostProblem::Unsupported),
    };
    response(service, run, outputs)
}

fn start(
    cursors: &BtsBrowseStore<'_>,
    owner: &BrowseOwner,
    request: &CicsRequest,
    scope: BrowseScope,
    items: Vec<BrowseItem>,
) -> Result<u32, HostProblem> {
    match effect(
        cursors,
        owner,
        request,
        BrowseEffect::Start { scope, items },
    )? {
        BrowseOutcome::Token(token) if token <= i32::MAX as u32 => Ok(token),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn effect(
    cursors: &BtsBrowseStore<'_>,
    owner: &BrowseOwner,
    request: &CicsRequest,
    effect: BrowseEffect,
) -> Result<BrowseOutcome, HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    cursors.apply(owner, mutation.idempotency_key.as_str(), digest, &effect)
}

fn inquiry_outputs(
    request: &CicsRequest,
    process: &super::super::bts_lifecycle::BtsProcess,
    activity: &BtsActivity,
) -> Result<Outputs, HostProblem> {
    if has(request, "ACTIVITY") && activity.name.len() > 16 {
        return Err(HostProblem::Unsupported);
    }
    let mut outputs = Vec::new();
    for (name, code) in [
        (
            "COMPSTATUS",
            match activity.completion {
                super::super::bts_lifecycle::BtsCompletion::Abend => 900,
                super::super::bts_lifecycle::BtsCompletion::Forced => 1013,
                super::super::bts_lifecycle::BtsCompletion::Incomplete => 1014,
                super::super::bts_lifecycle::BtsCompletion::Normal => 1016,
            },
        ),
        (
            "MODE",
            match activity.mode {
                super::super::bts_lifecycle::BtsMode::Active => 181,
                super::super::bts_lifecycle::BtsMode::Initial => 789,
                super::super::bts_lifecycle::BtsMode::Dormant => 1024,
                super::super::bts_lifecycle::BtsMode::Cancelling => 1025,
                super::super::bts_lifecycle::BtsMode::Complete => 1026,
            },
        ),
        ("SUSPSTATUS", if activity.suspended { 231 } else { 1027 }),
    ] {
        if has(request, name) {
            outputs.push((
                name,
                "mainframe-env.cics.decimal@1",
                code.to_string().into_bytes(),
            ));
        }
    }
    for (name, value, width) in [
        ("ABCODE", activity.abcode.as_deref().unwrap_or(""), 4),
        ("ABPROGRAM", activity.abprogram.as_deref().unwrap_or(""), 8),
        ("ACTIVITY", activity.name.as_str(), 16),
        (
            "EVENT",
            activity.completion_event.as_deref().unwrap_or(""),
            16,
        ),
        ("PROCESS", process.name.as_str(), 36),
        ("PROCESSTYPE", process.process_type.as_str(), 8),
        ("PROGRAM", activity.program.as_str(), 8),
        ("TRANSID", activity.transid.as_str(), 4),
        ("USERID", activity.userid.as_str(), 8),
    ] {
        if has(request, name) {
            outputs.push((name, "mainframe-env.cics.payload@1", padded(value, width)?));
        }
    }
    Ok(outputs)
}

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation == CicsOperation::BtsGetNextEvent
        && ["EVENTTYPE", "FIRESTATUS", "COMPOSITE", "PREDICATE", "TIMER"]
            .iter()
            .filter(|name| request.arguments.contains_key(**name))
            .count()
            > 1
    {
        return Err(HostProblem::Unsupported);
    }
    if request.operation == CicsOperation::BtsInquireActivity
        && ["COMPSTATUS", "MODE", "SUSPSTATUS"]
            .iter()
            .any(|name| request.arguments.contains_key(*name))
        && request
            .arguments
            .keys()
            .filter(|name| {
                !matches!(
                    name.as_str(),
                    "ACTIVITYID" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
                )
            })
            .count()
            != 1
    {
        return Err(HostProblem::Unsupported);
    }
    let (required_inputs, optional_inputs, required_outputs, optional_outputs): (
        &[&str],
        &[&str],
        &[&str],
        &[&str],
    ) = match request.operation {
        CicsOperation::BtsStartBrowseContainer => (
            &[],
            &["ACTIVITYID", "PROCESS", "PROCESSTYPE", "CHANNEL"],
            &["BROWSETOKEN"],
            &[],
        ),
        CicsOperation::BtsGetNextContainer => (&["BROWSETOKEN"], &[], &["CONTAINER"], &[]),
        CicsOperation::BtsEndBrowseContainer => (&["BROWSETOKEN"], &[], &[], &[]),
        CicsOperation::BtsInquireContainer => (
            &["CONTAINER"],
            &["ACTIVITYID", "PROCESS", "PROCESSTYPE", "SET.MAXLENGTH"],
            &[],
            &["DATALENGTH", "SET"],
        ),
        CicsOperation::BtsStartBrowseEvent => (&[], &["ACTIVITYID"], &["BROWSETOKEN"], &[]),
        CicsOperation::BtsStartBrowseTimer => (&["TIMER"], &["ACTIVITYID"], &["BROWSETOKEN"], &[]),
        CicsOperation::BtsGetNextEvent => (
            &["BROWSETOKEN"],
            &[],
            &["EVENT"],
            &["EVENTTYPE", "FIRESTATUS", "COMPOSITE", "PREDICATE", "TIMER"],
        ),
        CicsOperation::BtsEndBrowseEvent | CicsOperation::BtsEndBrowseTimer => {
            (&["BROWSETOKEN"], &[], &[], &[])
        }
        CicsOperation::BtsInquireEvent => (
            &["EVENT"],
            &["ACTIVITYID"],
            &[],
            &["EVENTTYPE", "FIRESTATUS", "COMPOSITE", "PREDICATE", "TIMER"],
        ),
        CicsOperation::BtsInquireTimer => (
            &["TIMER"],
            &["ACTIVITYID"],
            &[],
            &["EVENT", "STATUS", "ABSTIME"],
        ),
        CicsOperation::BtsStartBrowseProcess => (&["PROCESSTYPE"], &[], &["BROWSETOKEN"], &[]),
        CicsOperation::BtsStartBrowseActivity => (
            &[],
            &["ACTIVITYID", "PROCESS", "PROCESSTYPE"],
            &["BROWSETOKEN"],
            &[],
        ),
        CicsOperation::BtsGetNextProcess => (&["BROWSETOKEN"], &[], &["PROCESS"], &["ACTIVITYID"]),
        CicsOperation::BtsGetNextActivity => (
            &["BROWSETOKEN"],
            &[],
            &["ACTIVITY"],
            &["ACTIVITYID", "LEVEL"],
        ),
        CicsOperation::BtsEndBrowseProcess | CicsOperation::BtsEndBrowseActivity => {
            (&["BROWSETOKEN"], &[], &[], &[])
        }
        CicsOperation::BtsInquireProcess => {
            (&["PROCESS", "PROCESSTYPE"], &[], &[], &["ACTIVITYID"])
        }
        CicsOperation::BtsInquireActivity => (
            &["ACTIVITYID"],
            &[],
            &[],
            &[
                "ABCODE",
                "ABPROGRAM",
                "ACTIVITY",
                "COMPSTATUS",
                "EVENT",
                "MODE",
                "PROCESS",
                "PROCESSTYPE",
                "PROGRAM",
                "SUSPSTATUS",
                "TRANSID",
                "USERID",
            ],
        ),
        _ => return Err(HostProblem::Unsupported),
    };
    let has = |name: &str| request.arguments.contains_key(name);
    if required_inputs
        .iter()
        .chain(required_outputs)
        .any(|name| !has(name))
        || has("RESP2") && !has("RESP")
        || matches!(
            request.operation,
            CicsOperation::BtsStartBrowseContainer | CicsOperation::BtsInquireContainer
        ) && (has("PROCESS") != has("PROCESSTYPE")
            || has("ACTIVITYID") && has("PROCESS")
            || has("CHANNEL") && (has("ACTIVITYID") || has("PROCESS")))
        || request.operation == CicsOperation::BtsStartBrowseActivity
            && (has("PROCESS") != has("PROCESSTYPE") || has("ACTIVITYID") && has("PROCESS"))
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let role = if required_inputs.contains(&name.as_str())
            || optional_inputs.contains(&name.as_str())
        {
            if name == "BROWSETOKEN" || name == "SET.MAXLENGTH" {
                1
            } else {
                0
            }
        } else if required_outputs.contains(&name.as_str())
            || optional_outputs.contains(&name.as_str())
            || matches!(name.as_str(), "RESP" | "RESP2")
        {
            2
        } else if name == "OPTION.NOHANDLE" {
            3
        } else {
            return Err(HostProblem::Malformed);
        };
        let valid = match role {
            0 => matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ),
            1 => {
                value.schema() == "mainframe-env.cics.decimal@1"
                    && !value.bytes().is_empty()
                    && value.bytes().len() <= 10
                    && value.bytes().iter().all(u8::is_ascii_digit)
            }
            2 => value.schema() == "mainframe-env.cics.argument@1",
            _ => value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty(),
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if request.operation == CicsOperation::BtsInquireEvent
        && request.arguments.get("EVENT").is_some_and(|value| {
            std::str::from_utf8(value.bytes())
                .is_ok_and(|name| name.to_ascii_uppercase().starts_with("DFH"))
        })
    {
        return Err(HostProblem::Unsupported);
    }
    if request.operation == CicsOperation::BtsInquireContainer
        && request.arguments.contains_key("SET.MAXLENGTH") != request.arguments.contains_key("SET")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn text_arg(request: &CicsRequest, name: &str, max: usize) -> Result<String, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim_end_matches(' ');
    super::super::bts_lifecycle::validate_name(text, max, true)?;
    Ok(text.into())
}

fn token_arg(request: &CicsRequest) -> Result<u32, HostProblem> {
    let value = request
        .arguments
        .get("BROWSETOKEN")
        .ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
    text.parse::<u32>().map_err(|_| HostProblem::Malformed)
}

fn has(request: &CicsRequest, name: &str) -> bool {
    request.arguments.contains_key(name)
}

fn padded(value: &str, width: usize) -> Result<Vec<u8>, HostProblem> {
    if value.len() > width {
        return Err(HostProblem::Unsupported);
    }
    let mut bytes = value.as_bytes().to_vec();
    bytes.resize(width, b' ');
    Ok(bytes)
}

fn replay_outputs(request: &CicsRequest, outcome: BrowseOutcome) -> Result<Outputs, HostProblem> {
    match (request.operation, outcome) {
        (
            CicsOperation::BtsStartBrowseActivity
            | CicsOperation::BtsStartBrowseProcess
            | CicsOperation::BtsStartBrowseContainer,
            BrowseOutcome::Token(token),
        ) => Ok(vec![(
            "BROWSETOKEN",
            "mainframe-env.cics.decimal@1",
            token.to_string().into_bytes(),
        )]),
        (
            CicsOperation::BtsStartBrowseEvent | CicsOperation::BtsStartBrowseTimer,
            BrowseOutcome::Token(token),
        ) => Ok(vec![(
            "BROWSETOKEN",
            "mainframe-env.cics.decimal@1",
            token.to_string().into_bytes(),
        )]),
        (CicsOperation::BtsGetNextContainer, BrowseOutcome::Item(item)) => Ok(vec![(
            "CONTAINER",
            "mainframe-env.cics.payload@1",
            padded(&item.name, 16)?,
        )]),
        (CicsOperation::BtsGetNextEvent, BrowseOutcome::Item(item)) => {
            event_timer::event_outputs(request, &item)
        }
        (CicsOperation::BtsGetNextProcess, BrowseOutcome::Item(item)) => {
            let mut values = vec![(
                "PROCESS",
                "mainframe-env.cics.payload@1",
                padded(&item.name, 36)?,
            )];
            if has(request, "ACTIVITYID") {
                values.push((
                    "ACTIVITYID",
                    "mainframe-env.cics.payload@1",
                    padded(
                        item.activity_id
                            .as_deref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        52,
                    )?,
                ));
            }
            Ok(values)
        }
        (CicsOperation::BtsGetNextActivity, BrowseOutcome::Item(item)) => {
            let mut values = vec![(
                "ACTIVITY",
                "mainframe-env.cics.payload@1",
                padded(&item.name, 16)?,
            )];
            if has(request, "ACTIVITYID") {
                values.push((
                    "ACTIVITYID",
                    "mainframe-env.cics.payload@1",
                    padded(
                        item.activity_id
                            .as_deref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        52,
                    )?,
                ));
            }
            if has(request, "LEVEL") {
                values.push((
                    "LEVEL",
                    "mainframe-env.cics.decimal@1",
                    item.level.to_string().into_bytes(),
                ));
            }
            Ok(values)
        }
        (
            CicsOperation::BtsEndBrowseActivity
            | CicsOperation::BtsEndBrowseContainer
            | CicsOperation::BtsEndBrowseProcess
            | CicsOperation::BtsEndBrowseEvent
            | CicsOperation::BtsEndBrowseTimer,
            BrowseOutcome::Ended,
        ) => Ok(Vec::new()),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn response(
    service: &CicsService,
    run: &Run,
    outputs: Outputs,
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for (name, schema, bytes) in outputs {
        response.outputs.insert(
            name.into(),
            BoundedPayload::new(schema, bytes, InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::CicsConditionPolicy;

    #[test]
    fn bts_browse_getnext_event_metadata_profile_is_fenced_before_dispatch() {
        let argument = |schema: &str, bytes: &[u8]| {
            BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap()
        };
        for field in ["EVENTTYPE", "FIRESTATUS", "COMPOSITE", "PREDICATE", "TIMER"] {
            let request = CicsRequest {
                operation: CicsOperation::BtsGetNextEvent,
                arguments: [
                    (
                        "BROWSETOKEN".into(),
                        argument("mainframe-env.cics.decimal@1", b"1"),
                    ),
                    (
                        "EVENT".into(),
                        argument("mainframe-env.cics.argument@1", b"OUT"),
                    ),
                    (
                        field.into(),
                        argument("mainframe-env.cics.argument@1", b"OUT"),
                    ),
                ]
                .into(),
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            assert_eq!(validate(&request), Ok(()), "{field}");
            let mut combined = request.clone();
            combined.arguments.insert(
                "FIRESTATUS".into(),
                argument("mainframe-env.cics.argument@1", b"OUT"),
            );
            if field != "FIRESTATUS" {
                assert_eq!(validate(&combined), Err(HostProblem::Unsupported));
            }
        }
    }

    #[test]
    fn bts_browse_inquire_activity_cvda_forms_are_individually_admitted() {
        for field in ["COMPSTATUS", "MODE", "SUSPSTATUS"] {
            let request = CicsRequest {
                operation: CicsOperation::BtsInquireActivity,
                arguments: std::collections::BTreeMap::from([
                    (
                        "ACTIVITYID".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.literal@1",
                            b"A1".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                    (
                        field.into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.argument@1",
                            b"OUT".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                ]),
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            assert_eq!(validate(&request), Ok(()));
            let mut ambiguous = request.clone();
            ambiguous.arguments.insert(
                "ACTIVITY".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.argument@1",
                    b"OUT".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            assert_eq!(validate(&ambiguous), Err(HostProblem::Unsupported));
        }
    }

    #[test]
    fn bts_browse_inquire_activity_cvda_values_match_pinned_table() {
        use crate::service::handlers::bts_lifecycle::{BtsCompletion, BtsMode, BtsProcess};
        let id = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW").unwrap();
        let process = BtsProcess::new("TYPE", "ORDER", &id, "PROG", "BT01", "USER", "UOW").unwrap();
        let mut activity = process.activities[&id].clone();
        let argument = |schema: &str| {
            BoundedPayload::new(schema, b"X".to_vec(), InvocationLimits::default()).unwrap()
        };
        for (completion, expected) in [
            (BtsCompletion::Abend, "900"),
            (BtsCompletion::Forced, "1013"),
            (BtsCompletion::Incomplete, "1014"),
            (BtsCompletion::Normal, "1016"),
        ] {
            activity.completion = completion;
            let request = CicsRequest {
                operation: CicsOperation::BtsInquireActivity,
                arguments: [
                    (
                        "ACTIVITYID".into(),
                        argument("mainframe-env.cics.literal@1"),
                    ),
                    (
                        "COMPSTATUS".into(),
                        argument("mainframe-env.cics.argument@1"),
                    ),
                ]
                .into(),
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            assert_eq!(
                inquiry_outputs(&request, &process, &activity).unwrap()[0].2,
                expected.as_bytes()
            );
        }
        for (mode, expected) in [
            (BtsMode::Active, "181"),
            (BtsMode::Initial, "789"),
            (BtsMode::Dormant, "1024"),
            (BtsMode::Cancelling, "1025"),
            (BtsMode::Complete, "1026"),
        ] {
            activity.mode = mode;
            let request = CicsRequest {
                operation: CicsOperation::BtsInquireActivity,
                arguments: [
                    (
                        "ACTIVITYID".into(),
                        argument("mainframe-env.cics.literal@1"),
                    ),
                    ("MODE".into(), argument("mainframe-env.cics.argument@1")),
                ]
                .into(),
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            assert_eq!(
                inquiry_outputs(&request, &process, &activity).unwrap()[0].2,
                expected.as_bytes()
            );
        }
        for (suspended, expected) in [(false, "1027"), (true, "231")] {
            activity.suspended = suspended;
            let request = CicsRequest {
                operation: CicsOperation::BtsInquireActivity,
                arguments: [
                    (
                        "ACTIVITYID".into(),
                        argument("mainframe-env.cics.literal@1"),
                    ),
                    (
                        "SUSPSTATUS".into(),
                        argument("mainframe-env.cics.argument@1"),
                    ),
                ]
                .into(),
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            assert_eq!(
                inquiry_outputs(&request, &process, &activity).unwrap()[0].2,
                expected.as_bytes()
            );
        }
    }

    #[test]
    fn bts_browse_event_timer_unsupported_forms_fail_before_authority_access() {
        let argument = |bytes: &[u8]| {
            BoundedPayload::new(
                "mainframe-env.cics.argument@1",
                bytes.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        for (operation, arguments, expected) in [
            (
                CicsOperation::BtsInquireEvent,
                [("EVENT".into(), argument(b"DFHINITIAL"))].into(),
                HostProblem::Unsupported,
            ),
            (
                CicsOperation::BtsInquireTimer,
                [
                    ("TIMER".into(), argument(b"WAKE")),
                    (
                        "ABSTIME".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.literal@1",
                            b"OUT".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                ]
                .into(),
                HostProblem::Malformed,
            ),
        ] {
            let request = CicsRequest {
                operation,
                arguments,
                condition_policy: CicsConditionPolicy::Default,
                mutation: None,
            };
            assert_eq!(validate(&request), Err(expected));
        }
    }

    #[test]
    fn bts_browse_container_selector_and_pointer_shape_fail_before_authority_access() {
        let argument = |bytes: &[u8]| {
            BoundedPayload::new(
                "mainframe-env.cics.argument@1",
                bytes.to_vec(),
                InvocationLimits::default(),
            )
            .unwrap()
        };
        let request = |operation, arguments| CicsRequest {
            operation,
            arguments,
            condition_policy: CicsConditionPolicy::Default,
            mutation: None,
        };
        assert_eq!(
            validate(&request(
                CicsOperation::BtsStartBrowseContainer,
                [
                    ("CHANNEL".into(), argument(b"CH")),
                    ("PROCESS".into(), argument(b"P")),
                    ("PROCESSTYPE".into(), argument(b"TYPE")),
                    ("BROWSETOKEN".into(), argument(b"OUT")),
                ]
                .into(),
            )),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            validate(&request(
                CicsOperation::BtsInquireContainer,
                [
                    ("CONTAINER".into(), argument(b"ITEM")),
                    ("OPTION.ACQPROCESS".into(), argument(b"")),
                ]
                .into(),
            )),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            validate(&request(
                CicsOperation::BtsInquireContainer,
                [
                    ("CONTAINER".into(), argument(b"ITEM")),
                    ("SET".into(), argument(b"OUT")),
                ]
                .into(),
            )),
            Err(HostProblem::Malformed)
        );
    }
}
