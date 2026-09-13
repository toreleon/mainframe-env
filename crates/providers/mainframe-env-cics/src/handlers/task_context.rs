use super::super::{CicsService, Run, bounded, decimal_payload};
use mainframe_env_execution_api::{Invocation, RunUnitId};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use std::collections::BTreeMap;

pub(in crate::service) fn current_program(invocation: &Invocation) -> Option<String> {
    invocation
        .selector
        .as_str()
        .strip_prefix("program:")
        .filter(|program| !program.is_empty())
        .map(str::to_ascii_uppercase)
}

pub(in crate::service) fn synchronize_current_program(
    runs: &mut BTreeMap<RunUnitId, Run>,
    invocation: &Invocation,
) -> Result<bool, HostProblem> {
    let Some(run) = runs.get_mut(&invocation.run_unit_id) else {
        return Ok(false);
    };
    if run.invocation.principal.id() != invocation.principal.id() {
        return Err(HostProblem::Unauthorized);
    }
    if let Some(program) = current_program(invocation) {
        run.current_program = Some(program);
    }
    Ok(true)
}

pub(in crate::service) fn assign(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_assign_request(request)?;
    let dpl = assign_dpl_context(run)?;
    let prohibited = dpl
        && ["NEXTTRANSID", "OPSECURITY", "TCTUALENG"]
            .iter()
            .any(|name| request.arguments.contains_key(*name));
    let mut response = if prohibited {
        super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 200,
            },
        )?
    } else {
        service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?
    };
    for (name, value) in [
        ("APPLID", run.applid.as_bytes()),
        ("SYSID", run.sysid.as_bytes()),
        ("USERID", run.invocation.principal.id().as_str().as_bytes()),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(value.to_vec())?);
        }
    }
    if request.arguments.contains_key("TASKPRIORITY") {
        response.outputs.insert(
            "TASKPRIORITY".into(),
            decimal_payload(i64::from(run.invocation.priority))?,
        );
    }
    if request.arguments.contains_key("PROGRAM") {
        let program = run
            .current_program
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        response
            .outputs
            .insert("PROGRAM".into(), bounded(program.as_bytes().to_vec())?);
    }
    for (name, length) in [
        ("APPLICATION", 64),
        ("CHANNEL", 16),
        ("OPERATION", 64),
        ("PLATFORM", 64),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(vec![b' '; length])?);
        }
    }
    for name in ["MAJORVERSION", "MICROVERSION", "MINORVERSION"] {
        if request.arguments.contains_key(name) {
            response.outputs.insert(name.into(), decimal_payload(-1)?);
        }
    }
    for name in ["CWALENG", "TWALENG"] {
        if request.arguments.contains_key(name) {
            response.outputs.insert(name.into(), decimal_payload(0)?);
        }
    }
    if !prohibited && request.arguments.contains_key("NEXTTRANSID") {
        response
            .outputs
            .insert("NEXTTRANSID".into(), bounded(vec![b' '; 4])?);
    }
    if request.arguments.contains_key("INITPARMLEN") {
        response
            .outputs
            .insert("INITPARMLEN".into(), decimal_payload(0)?);
    }
    // With no configured INITPARM for the current program, IBM leaves the
    // INITPARM receiver unchanged; omitting that output preserves its bytes.
    for (name, value) in [("OPERKEYS", vec![0; 8]), ("RESTART", vec![0])] {
        if request.arguments.contains_key(name) {
            response.outputs.insert(name.into(), bounded(value)?);
        }
    }
    if !prohibited && request.arguments.contains_key("TCTUALENG") {
        response
            .outputs
            .insert("TCTUALENG".into(), decimal_payload(0)?);
    }
    if !prohibited && request.arguments.contains_key("OPSECURITY") {
        response
            .outputs
            .insert("OPSECURITY".into(), bounded(vec![0; 3])?);
    }
    Ok(response)
}

fn assign_dpl_context(run: &Run) -> Result<bool, HostProblem> {
    let Some(context) = run.invocation.bindings.get("cics.execution-context") else {
        return Ok(false);
    };
    if context.schema() != "mainframe-env.cics.execution-context@1" {
        return Err(HostProblem::Malformed);
    }
    match context.bytes() {
        b"local" => Ok(false),
        b"dpl-synconreturn" | b"dpl-without-synconreturn" | b"dpl-executionset-subset" => Ok(true),
        _ => Err(HostProblem::Malformed),
    }
}

fn validate_assign_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "APPLICATION",
        "APPLID",
        "CHANNEL",
        "CWALENG",
        "INITPARM",
        "INITPARMLEN",
        "MAJORVERSION",
        "MICROVERSION",
        "MINORVERSION",
        "NEXTTRANSID",
        "OPTION.NOHANDLE",
        "OPERATION",
        "OPERKEYS",
        "OPSECURITY",
        "PLATFORM",
        "PROGRAM",
        "RESP",
        "RESP2",
        "RESTART",
        "SYSID",
        "TASKPRIORITY",
        "TCTUALENG",
        "TWALENG",
        "USERID",
    ];
    if request.arguments.len() > 16
        || request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || if name == "OPTION.NOHANDLE" {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                } else {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
