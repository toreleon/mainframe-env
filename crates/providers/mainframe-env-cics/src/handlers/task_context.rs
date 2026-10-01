use super::super::{CicsService, Run, Session, bounded, decimal_payload};
use mainframe_env_execution_api::{ExecutionId, Invocation};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use std::collections::{BTreeMap, BTreeSet};

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

const LOCAL_CCSID: i64 = 37;
const BMS_OVERFLOW_OPTIONS: [&str; 5] = ["DESTCOUNT", "LDCMNEM", "LDCNUM", "PAGENUM", "PARTNPAGE"];

pub(in crate::service) fn allocate_terminal_input(
    sessions: &BTreeMap<String, Session>,
) -> Result<super::TerminalInput, HostProblem> {
    const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let used = sessions
        .values()
        .filter_map(|session| session.input.terminal_id.as_deref())
        .collect::<BTreeSet<_>>();
    for ordinal in 0..36_usize.pow(3) {
        let candidate = [
            b'T',
            DIGITS[(ordinal / (36 * 36)) % 36],
            DIGITS[(ordinal / 36) % 36],
            DIGITS[ordinal % 36],
        ];
        let candidate = String::from_utf8(candidate.to_vec()).expect("ASCII terminal identifier");
        if !used.contains(candidate.as_str()) {
            return Ok(super::TerminalInput::identified(candidate));
        }
    }
    Err(HostProblem::ResourceExhausted)
}

#[derive(Clone)]
pub(in crate::service) struct CurrentProgramFrame {
    pub(in crate::service) effect_invocation: Invocation,
    pub(in crate::service) logical_level: u32,
    pub(in crate::service) invoking_program: Option<String>,
    pub(in crate::service) return_program: Option<String>,
    pub(in crate::service) current: Option<String>,
    pub(in crate::service) channel: Option<String>,
    pub(in crate::service) parent_execution_id: Option<ExecutionId>,
    pub(in crate::service) initial_entry: bool,
}

pub(in crate::service) fn current_program(invocation: &Invocation) -> Option<String> {
    invocation
        .selector
        .as_str()
        .strip_prefix("program:")
        .filter(|program| !program.is_empty())
        .map(str::to_ascii_uppercase)
}

pub(in crate::service) fn current_channel(invocation: &Invocation) -> Option<String> {
    invocation
        .bindings
        .get("cics.channel")
        .filter(|value| value.schema() == "mainframe-env.cics.channel@1")
        .and_then(|value| std::str::from_utf8(value.bytes()).ok())
        .filter(|name| super::bts_container::valid_task_channel_name(name))
        .map(str::to_owned)
}

pub(in crate::service) fn synchronize_current_program(
    state: &mut super::super::State,
    invocation: &Invocation,
) -> Result<bool, HostProblem> {
    let initial_entry = initial_program_entry(invocation)?;
    super::host_boundary::admit_frame(state, invocation)?;
    let Some(run) = state.runs.get_mut(&invocation.run_unit_id) else {
        return Ok(false);
    };
    if run.invocation.principal.id() != invocation.principal.id() {
        return Err(HostProblem::Unauthorized);
    }
    run.current_program.parent_execution_id = invocation.parent_execution_id.clone();
    run.current_program.initial_entry = initial_entry && run.current_program.logical_level == 1;
    run.current_program.effect_invocation = invocation.clone();
    let next_program = current_program(invocation);
    if next_program != run.current_program.current
        || invocation.bindings.contains_key("cics.channel")
    {
        run.current_program.channel = current_channel(invocation);
    }
    if let Some(program) = next_program {
        run.current_program.current = Some(program);
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
    let map_geometry_requested = ["MAPCOLUMN", "MAPHEIGHT", "MAPLINE", "MAPWIDTH"]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let input_partition_requested = request.arguments.contains_key("INPARTN");
    let terminal_identity_requested = ["FACILITY", "NETNAME"]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let terminal_address_requested = request.arguments.contains_key("TNADDR");
    let terminal_required = screen_requested
        || terminal_indicator_requested
        || input_partition_requested
        || terminal_identity_requested
        || terminal_address_requested
        || request.arguments.contains_key("PARTNSET")
        || request.arguments.contains_key("TERMPRIORITY")
        || map_geometry_requested;
    let dimensions = if !dpl && (terminal_required || request.arguments.contains_key("FCI")) {
        terminal_dimensions(service, run)?
    } else {
        None
    };
    let terminal_missing = !dpl && terminal_required && dimensions.is_none();
    let map_geometry = if !dpl && map_geometry_requested && dimensions.is_some() {
        positioned_map_geometry(service, run)?
    } else {
        None
    };
    let map_missing =
        !dpl && map_geometry_requested && dimensions.is_some() && map_geometry.is_none();
    let input_partition = if !dpl && input_partition_requested {
        terminal_has_positioned_map(service, run)?
    } else {
        None
    };
    if input_partition == Some(true) {
        return Err(HostProblem::InfrastructureFailure);
    }
    let input_partition_missing = input_partition == Some(false);
    let terminal_identity = if !dpl && terminal_identity_requested && dimensions.is_some() {
        terminal_identity(service, run)?
    } else {
        None
    };
    if !dpl && terminal_identity_requested && dimensions.is_some() && terminal_identity.is_none() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let intersystem_facility_missing = request.arguments.contains_key("PRINSYSID");
    let ati_missing = !dpl && request.arguments.contains_key("QNAME");
    let bts_missing = ["ACTIVITY", "ACTIVITYID", "PROCESS", "PROCESSTYPE"]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let bdi_requested = ["DESTID", "DESTIDLENG"]
        .iter()
        .any(|name| request.arguments.contains_key(*name));
    let bdi_destination = if !dpl && bdi_requested {
        service.outboard_last_inbound_destination(&run.invocation.run_unit_id)?
    } else {
        None
    };
    let bdi_missing = !dpl && bdi_requested && bdi_destination.is_none();
    let bms_overflow_missing = !dpl
        && BMS_OVERFLOW_OPTIONS
            .iter()
            .any(|name| request.arguments.contains_key(*name));
    let link_level = request
        .arguments
        .contains_key("LINKLEVEL")
        .then(|| assign_link_level(run, dpl))
        .transpose()?;
    if request.arguments.contains_key("INVOKINGPROG") {
        validate_initial_program(run, dpl)?;
    }
    if request.arguments.contains_key("RETURNPROG") {
        validate_top_level_return_program(run, dpl)?;
    }
    let dpl_prohibited = dpl
        && (terminal_indicator_requested
            || [
                "ALTSCRNHT",
                "ALTSCRNWD",
                "DEFSCRNHT",
                "DEFSCRNWD",
                "DESTID",
                "DESTIDLENG",
                "DESTCOUNT",
                "FACILITY",
                "FCI",
                "INPARTN",
                "MAPCOLUMN",
                "MAPHEIGHT",
                "MAPLINE",
                "MAPWIDTH",
                "LDCMNEM",
                "LDCNUM",
                "NEXTTRANSID",
                "OPSECURITY",
                "PARTNSET",
                "PAGENUM",
                "PARTNPAGE",
                "QNAME",
                "SCRNHT",
                "SCRNWD",
                "TCTUALENG",
                "TERMPRIORITY",
            ]
            .iter()
            .any(|name| request.arguments.contains_key(*name)));
    if dpl
        && ["NETNAME", "TNADDR"]
            .iter()
            .any(|name| request.arguments.contains_key(*name))
        && !dpl_prohibited
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut response = if dpl_prohibited
        || terminal_missing
        || map_missing
        || input_partition_missing
        || intersystem_facility_missing
        || ati_missing
        || bts_missing
        || bdi_missing
        || bms_overflow_missing
    {
        super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: if dpl_prohibited {
                    200
                } else if terminal_missing || intersystem_facility_missing {
                    5
                } else if map_missing || bms_overflow_missing || input_partition_missing {
                    2
                } else if ati_missing {
                    4
                } else if bts_missing {
                    6
                } else {
                    3
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
    if request.arguments.contains_key("INPUTMSGLEN") {
        response.outputs.insert(
            "INPUTMSGLEN".into(),
            decimal_payload(session_input_message_length(service, run)?)?,
        );
    }
    if let Some(destination) = bdi_destination {
        if request.arguments.contains_key("DESTID") {
            let mut bytes = destination.as_bytes().to_vec();
            bytes.resize(8, b' ');
            response.outputs.insert("DESTID".into(), bounded(bytes)?);
        }
        if request.arguments.contains_key("DESTIDLENG") {
            response.outputs.insert(
                "DESTIDLENG".into(),
                decimal_payload(destination.len() as i64)?,
            );
        }
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
    if request.arguments.contains_key("ORGABCODE") {
        let mut value = vec![b' '; 4];
        if let Some(record) = &run.latest_abend {
            value[..record.original_code.len()].copy_from_slice(&record.original_code);
        }
        response.outputs.insert("ORGABCODE".into(), bounded(value)?);
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
    if request.arguments.contains_key("LOCALCCSID") {
        response
            .outputs
            .insert("LOCALCCSID".into(), decimal_payload(LOCAL_CCSID)?);
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
        if request.arguments.contains_key("TERMPRIORITY") {
            response
                .outputs
                .insert("TERMPRIORITY".into(), decimal_payload(0)?);
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
    if let Some((line, column, rows, columns)) = map_geometry {
        for (name, value) in [
            ("MAPCOLUMN", column),
            ("MAPHEIGHT", rows),
            ("MAPLINE", line),
            ("MAPWIDTH", columns),
        ] {
            if request.arguments.contains_key(name) {
                response
                    .outputs
                    .insert(name.into(), decimal_payload(i64::from(value))?);
            }
        }
    }
    if let Some(terminal_id) = terminal_identity {
        if request.arguments.contains_key("FACILITY") {
            response
                .outputs
                .insert("FACILITY".into(), bounded(terminal_id.as_bytes().to_vec())?);
        }
        if request.arguments.contains_key("NETNAME") {
            let mut netname = terminal_id.into_bytes();
            netname.resize(8, b' ');
            response.outputs.insert("NETNAME".into(), bounded(netname)?);
        }
    }
    if dimensions.is_some() && request.arguments.contains_key("TNADDR") {
        response
            .outputs
            .insert("TNADDR".into(), bounded(vec![b' '; 39])?);
    }
    if !dpl_prohibited && request.arguments.contains_key("FCI") {
        response
            .outputs
            .insert("FCI".into(), bounded(vec![u8::from(dimensions.is_some())])?);
    }
    if request.arguments.contains_key("PROGRAM") {
        let program = run
            .current_program
            .current
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        response
            .outputs
            .insert("PROGRAM".into(), bounded(program.as_bytes().to_vec())?);
    }
    for (name, program) in [
        (
            "INVOKINGPROG",
            run.current_program.invoking_program.as_deref(),
        ),
        ("RETURNPROG", run.current_program.return_program.as_deref()),
    ] {
        if request.arguments.contains_key(name) {
            let mut value = program.unwrap_or("").as_bytes().to_vec();
            if value.len() > 8 {
                return Err(HostProblem::InfrastructureFailure);
            }
            value.resize(8, b' ');
            response.outputs.insert(name.into(), bounded(value)?);
        }
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
    if request.arguments.contains_key("LANGINUSE") {
        response
            .outputs
            .insert("LANGINUSE".into(), bounded(b"ENU".to_vec())?);
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

fn session_input_message_length(service: &CicsService, run: &Run) -> Result<i64, HostProblem> {
    let state = service.lock()?;
    let session = state
        .sessions
        .get(&run.session)
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok(i64::from(session.input.message_length))
}

fn positioned_map_geometry(
    service: &CicsService,
    run: &Run,
) -> Result<Option<(u16, u16, u16, u16)>, HostProblem> {
    let state = service.lock()?;
    let session = state
        .sessions
        .get(&run.session)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if session.principal != run.invocation.principal.id().as_str()
        || session.run_unit != run.invocation.run_unit_id.as_str()
        || session.transaction != run.transaction
    {
        return Ok(None);
    }
    let Some((mapset, map)) = session.mapset.as_ref().zip(session.map.as_ref()) else {
        return Ok(None);
    };
    let definition = state
        .maps
        .get(&(mapset.clone(), map.clone()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok(Some((
        definition.line,
        definition.column,
        definition.rows,
        definition.columns,
    )))
}

fn terminal_has_positioned_map(
    service: &CicsService,
    run: &Run,
) -> Result<Option<bool>, HostProblem> {
    let state = service.lock()?;
    let session = state
        .sessions
        .get(&run.session)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if session.principal != run.invocation.principal.id().as_str()
        || session.run_unit != run.invocation.run_unit_id.as_str()
        || session.transaction != run.transaction
    {
        return Ok(None);
    }
    match (session.mapset.is_some(), session.map.is_some()) {
        (false, false) => Ok(Some(false)),
        (true, true) => Ok(Some(true)),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn terminal_identity(service: &CicsService, run: &Run) -> Result<Option<String>, HostProblem> {
    let state = service.lock()?;
    let session = state
        .sessions
        .get(&run.session)
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok((session.principal == run.invocation.principal.id().as_str()
        && session.run_unit == run.invocation.run_unit_id.as_str()
        && session.transaction == run.transaction)
        .then(|| session.input.terminal_id.clone())
        .flatten())
}

fn assign_link_level(run: &Run, dpl: bool) -> Result<i64, HostProblem> {
    if run.current_program.logical_level > 1 {
        Ok(i64::from(run.current_program.logical_level) + i64::from(dpl))
    } else if dpl {
        Ok(2)
    } else if run.invocation.parent_execution_id.is_none() {
        Ok(1)
    } else {
        Err(HostProblem::InfrastructureFailure)
    }
}

fn validate_initial_program(run: &Run, dpl: bool) -> Result<(), HostProblem> {
    if run.current_program.logical_level > 1 && run.current_program.invoking_program.is_some() {
        return Ok(());
    }
    if dpl
        || run.current_program.parent_execution_id.is_some()
        || !run.current_program.initial_entry
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn validate_top_level_return_program(run: &Run, dpl: bool) -> Result<(), HostProblem> {
    if run.current_program.logical_level > 1 && run.current_program.return_program.is_some() {
        return Ok(());
    }
    if dpl || run.current_program.parent_execution_id.is_some() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn initial_program_entry(invocation: &Invocation) -> Result<bool, HostProblem> {
    let Some(entry) = invocation.bindings.get("cics.program-entry") else {
        return Ok(false);
    };
    if entry.schema() != "mainframe-env.cics.program-entry@1" {
        return Err(HostProblem::Malformed);
    }
    match entry.bytes() {
        b"initial" => Ok(true),
        b"xctl" => Ok(false),
        _ => Err(HostProblem::Malformed),
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
        "DESTID",
        "DESTIDLENG",
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
        "LOCALCCSID",
        "MAJORVERSION",
        "MAPCOLUMN",
        "MAPHEIGHT",
        "MAPLINE",
        "MAPWIDTH",
        "MICROVERSION",
        "MINORVERSION",
        "MSRCONTROL",
        "NEXTTRANSID",
        "OPTION.NOHANDLE",
        "OPERATION",
        "OPERKEYS",
        "OPSECURITY",
        "ORGABCODE",
        "OUTLINE",
        "PARTNS",
        "PARTNSET",
        "PLATFORM",
        "PRINSYSID",
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
        "DESTCOUNT",
        "LDCMNEM",
        "LDCNUM",
        "PAGENUM",
        "PARTNPAGE",
        "RETURNPROG",
        "TERMPRIORITY",
        "LANGINUSE",
        "INPUTMSGLEN",
        "INVOKINGPROG",
        "INPARTN",
        "FACILITY",
        "NETNAME",
        "TNADDR",
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
