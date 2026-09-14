use super::super::{CicsService, Run, bounded, decimal_payload};
use mainframe_env_execution_api::{Invocation, RunUnitId};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use std::collections::BTreeMap;

const TERMINAL_CAPABILITY_INDICATORS: [(&str, u8); 20] = [
    ("APLKYBD", 0x00),
    ("APLTEXT", 0x00),
    ("BTRANS", 0x00),
    ("COLOR", 0x00),
    ("DS3270", 0xff),
    ("DSSCS", 0x00),
    ("EWASUPP", 0x00),
    ("EXTDS", 0x00),
    ("GMMI", 0x00),
    ("HILIGHT", 0x00),
    ("KATAKANA", 0x00),
    ("MSRCONTROL", 0x00),
    ("OUTLINE", 0x00),
    ("PARTNS", 0x00),
    ("PS", 0x00),
    ("SOSI", 0x00),
    ("TEXTKYBD", 0x00),
    ("TEXTPRINT", 0x00),
    ("UNATTEND", 0x00),
    ("VALIDATION", 0x00),
];

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
    let screen_options = [
        "ALTSCRNHT",
        "ALTSCRNWD",
        "DEFSCRNHT",
        "DEFSCRNWD",
        "SCRNHT",
        "SCRNWD",
    ];
    let screen_requested = screen_options
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let terminal_indicator_requested = TERMINAL_CAPABILITY_INDICATORS
        .iter()
        .any(|(name, _)| request.arguments.contains_key(*name));
    let terminal_required = screen_requested
        || terminal_indicator_requested
        || request.arguments.contains_key("PARTNSET");
    let dimensions = if !dpl && (terminal_required || request.arguments.contains_key("FCI")) {
        terminal_dimensions(service, run)?
    } else {
        None
    };
    let terminal_missing = !dpl && terminal_required && dimensions.is_none();
    let ati_missing = !dpl && request.arguments.contains_key("QNAME");
    let bts_missing = ["ACTIVITY", "ACTIVITYID", "PROCESS", "PROCESSTYPE"]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let link_level = request
        .arguments
        .contains_key("LINKLEVEL")
        .then(|| assign_link_level(run, dpl))
        .transpose()?;
    let dpl_prohibited = dpl
        && (terminal_indicator_requested
            || [
                "ALTSCRNHT",
                "ALTSCRNWD",
                "DEFSCRNHT",
                "DEFSCRNWD",
                "FCI",
                "NEXTTRANSID",
                "OPSECURITY",
                "PARTNSET",
                "QNAME",
                "SCRNHT",
                "SCRNWD",
                "TCTUALENG",
            ]
            .iter()
            .any(|name| request.arguments.contains_key(*name)));
    let mut response = if dpl_prohibited || terminal_missing || ati_missing || bts_missing {
        super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: if dpl_prohibited {
                    200
                } else if terminal_missing {
                    5
                } else if ati_missing {
                    4
                } else {
                    6
                },
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
    if request.arguments.contains_key("ABCODE") {
        let mut value = vec![b' '; 4];
        if let Some(record) = &run.latest_abend {
            value[..record.code.len()].copy_from_slice(&record.code);
        }
        response.outputs.insert("ABCODE".into(), bounded(value)?);
    }
    if request.arguments.contains_key("ABDUMP") {
        let value = run
            .latest_abend
            .as_ref()
            .is_some_and(|record| record.dump_requested);
        response.outputs.insert(
            "ABDUMP".into(),
            bounded(vec![if value { 0xff } else { 0 }])?,
        );
    }
    if request.arguments.contains_key("ABPROGRAM") {
        let value = run
            .latest_abend
            .as_ref()
            .and_then(|record| record.program.as_ref())
            .map_or_else(|| vec![0; 8], |program| program.as_bytes().to_vec());
        response.outputs.insert("ABPROGRAM".into(), bounded(value)?);
    }
    if request.arguments.contains_key("ERRORMSG") {
        response
            .outputs
            .insert("ERRORMSG".into(), bounded(vec![0; 500])?);
    }
    if request.arguments.contains_key("ERRORMSGLEN") {
        response
            .outputs
            .insert("ERRORMSGLEN".into(), decimal_payload(0)?);
    }
    if let Some(link_level) = link_level {
        response
            .outputs
            .insert("LINKLEVEL".into(), decimal_payload(link_level)?);
    }
    if let Some((rows, columns)) = dimensions {
        for (name, value) in [
            ("ALTSCRNHT", rows),
            ("ALTSCRNWD", columns),
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
        for (name, value) in TERMINAL_CAPABILITY_INDICATORS {
            if request.arguments.contains_key(name) {
                response.outputs.insert(name.into(), bounded(vec![value])?);
            }
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
    for name in ["CMDSEC", "RESSEC"] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(b"X".to_vec())?);
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
        ("ASRAINTRPT", 8),
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
        "ABCODE",
        "ABDUMP",
        "ABOFFSET",
        "ABPROGRAM",
        "ACTIVITY",
        "ACTIVITYID",
        "ALTSCRNHT",
        "ALTSCRNWD",
        "APLKYBD",
        "APLTEXT",
        "APPLICATION",
        "APPLID",
        "ASRAINTRPT",
        "ASRAPSW",
        "ASRAPSW16",
        "ASRAREGS",
        "ASRAREGS64",
        "BRIDGE",
        "BTRANS",
        "CHANNEL",
        "CMDSEC",
        "COLOR",
        "CWALENG",
        "DEFSCRNHT",
        "DEFSCRNWD",
        "DS3270",
        "DSSCS",
        "ERRORMSG",
        "ERRORMSGLEN",
        "EWASUPP",
        "EXTDS",
        "FCI",
        "GMMI",
        "HILIGHT",
        "INITPARM",
        "INITPARMLEN",
        "KATAKANA",
        "LINKLEVEL",
        "MAJORVERSION",
        "MICROVERSION",
        "MINORVERSION",
        "MSRCONTROL",
        "NEXTTRANSID",
        "OPTION.NOHANDLE",
        "OPERATION",
        "OPERKEYS",
        "OPSECURITY",
        "OUTLINE",
        "PARTNS",
        "PARTNSET",
        "PLATFORM",
        "PROCESS",
        "PROCESSTYPE",
        "PROGRAM",
        "PS",
        "QNAME",
        "RESP",
        "RESP2",
        "RESSEC",
        "RESTART",
        "SCRNHT",
        "SCRNWD",
        "SOSI",
        "SYSID",
        "TASKPRIORITY",
        "TCTUALENG",
        "TEXTKYBD",
        "TEXTPRINT",
        "TWALENG",
        "UNATTEND",
        "USERID",
        "VALIDATION",
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
