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
    let screen_options = ["DEFSCRNHT", "DEFSCRNWD", "SCRNHT", "SCRNWD"];
    let screen_requested = screen_options
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let terminal_required = screen_requested || request.arguments.contains_key("PARTNSET");
    let dimensions = if !dpl && (terminal_required || request.arguments.contains_key("FCI")) {
        terminal_dimensions(service, run)?
    } else {
        None
    };
    let terminal_missing = !dpl && terminal_required && dimensions.is_none();
    let link_level = request
        .arguments
        .contains_key("LINKLEVEL")
        .then(|| assign_link_level(run, dpl))
        .transpose()?;
    let dpl_prohibited = dpl
        && [
            "DEFSCRNHT",
            "DEFSCRNWD",
            "FCI",
            "NEXTTRANSID",
            "OPSECURITY",
            "PARTNSET",
            "SCRNHT",
            "SCRNWD",
            "TCTUALENG",
        ]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let mut response = if dpl_prohibited || terminal_missing {
        super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: if dpl_prohibited { 200 } else { 0 },
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
    if request.arguments.contains_key("ABOFFSET") {
        response
            .outputs
            .insert("ABOFFSET".into(), decimal_payload(0)?);
    }
    if let Some(link_level) = link_level {
        response
            .outputs
            .insert("LINKLEVEL".into(), decimal_payload(link_level)?);
    }
    if let Some((rows, columns)) = dimensions {
        for (name, value) in [
            ("DEFSCRNHT", rows),
            ("DEFSCRNWD", columns),
            ("SCRNHT", rows),
            ("SCRNWD", columns),
        ] {
            if request.arguments.contains_key(name) {
                response
                    .outputs
                    .insert(name.into(), decimal_payload(i64::from(value))?);
            }
        }
        if request.arguments.contains_key("PARTNSET") {
            response
                .outputs
                .insert("PARTNSET".into(), bounded(vec![b' '; 6])?);
        }
    }
    if !dpl_prohibited && request.arguments.contains_key("FCI") {
        response
            .outputs
            .insert("FCI".into(), bounded(vec![u8::from(dimensions.is_some())])?);
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
        ("BRIDGE", 4),
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
    if !dpl_prohibited && request.arguments.contains_key("NEXTTRANSID") {
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
    for (name, length) in [
        ("ASRAPSW", 8),
        ("ASRAPSW16", 16),
        ("ASRAREGS", 64),
        ("ASRAREGS64", 128),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(vec![0; length])?);
        }
    }
    if !dpl_prohibited && request.arguments.contains_key("TCTUALENG") {
        response
            .outputs
            .insert("TCTUALENG".into(), decimal_payload(0)?);
    }
    if !dpl_prohibited && request.arguments.contains_key("OPSECURITY") {
        response
            .outputs
            .insert("OPSECURITY".into(), bounded(vec![0; 3])?);
    }
    Ok(response)
}

fn terminal_dimensions(
    service: &CicsService,
    run: &Run,
) -> Result<Option<(u16, u16)>, HostProblem> {
    let state = service.lock()?;
    let session = state
        .sessions
        .get(&run.session)
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok((session.principal == run.invocation.principal.id().as_str()
        && session.run_unit == run.invocation.run_unit_id.as_str()
        && session.transaction == run.transaction)
        .then_some((session.rows, session.columns)))
}

fn assign_link_level(run: &Run, dpl: bool) -> Result<i64, HostProblem> {
    if dpl {
        Ok(2)
    } else if run.invocation.parent_execution_id.is_none() {
        Ok(1)
    } else {
        Err(HostProblem::InfrastructureFailure)
    }
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
        "ABOFFSET",
        "APPLICATION",
        "APPLID",
        "ASRAPSW",
        "ASRAPSW16",
        "ASRAREGS",
        "ASRAREGS64",
        "BRIDGE",
        "CHANNEL",
        "CWALENG",
        "DEFSCRNHT",
        "DEFSCRNWD",
        "FCI",
        "INITPARM",
        "INITPARMLEN",
        "LINKLEVEL",
        "MAJORVERSION",
        "MICROVERSION",
        "MINORVERSION",
        "NEXTTRANSID",
        "OPTION.NOHANDLE",
        "OPERATION",
        "OPERKEYS",
        "OPSECURITY",
        "PARTNSET",
        "PLATFORM",
        "PROGRAM",
        "RESP",
        "RESP2",
        "RESTART",
        "SCRNHT",
        "SCRNWD",
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
