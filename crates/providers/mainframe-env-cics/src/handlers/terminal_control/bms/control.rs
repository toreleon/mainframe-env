use super::super::partition_set::{normalized_name, partition_exists, read_association};
use super::*;
use mainframe_env_host_api::{AccessIntent, CicsOperation};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    super::super::validate_purge_message_context(run)?;
    if let Some(response) = receipt_for_request(service, run, request)? {
        return Ok(response);
    }
    let current = service
        .lock()?
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let mut frame = frame(request, &current)?;
    let selected_set = read_association(service, run.invocation.run_unit_id.as_str())?
        .and_then(|association| association.name);
    if selected_set.is_some() {
        for name in [frame.outpartn.as_deref(), frame.actpartn.as_deref()]
            .into_iter()
            .flatten()
        {
            if !partition_exists(service, run, name)? {
                return Err(condition("INVPARTN", 65));
            }
        }
    } else {
        // IBM ignores partition controls when no application set is active.
        frame.outpartn = None;
        frame.actpartn = None;
    }
    if request.arguments.contains_key("LDC") {
        return Err(condition("INVLDC", 41));
    }
    service.authorize(
        run,
        "FACILITY",
        "CICS.TERMINAL.CONTROL",
        AccessIntent::Update,
    )?;
    let mut state = read_state(service, &run.session)?;
    let mut receipt = base_receipt(run, request)?;
    if has(request, "ACCUM") {
        let mode = if has(request, "PAGING") {
            1
        } else if request.arguments.contains_key("SET") {
            2
        } else {
            0
        };
        let requested_reqid = request
            .arguments
            .get("REQID")
            .map(|value| normalize_reqid(value.bytes()))
            .transpose()?
            .unwrap_or_else(|| {
                state
                    .message
                    .as_ref()
                    .map(|message| message.reqid.clone())
                    .unwrap_or_else(|| "**".into())
            });
        let message = state.message.get_or_insert_with(|| LogicalMessage {
            mode,
            reqid: requested_reqid.clone(),
            frames: Vec::new(),
            payload: Vec::new(),
        });
        if message.mode != mode {
            return Err(condition("INVREQ", 16));
        }
        if message.reqid != requested_reqid {
            return Err(condition("IGREQID", 39));
        }
        if message.frames.len() >= service.limits.max_fields {
            return Err(HostProblem::ResourceExhausted);
        }
        message.frames.push(frame);
        commit(service, run, request, None, None, &state, &receipt)
    } else {
        if state.message.is_some() {
            return Err(condition("INVREQ", 16));
        }
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        apply_frame(&mut state, &mut next, &frame)?;
        state.last_page = next.screen.clone();
        if request.arguments.contains_key("SET") {
            let capacity = decimal(request, "SET.MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
            let set = page_pointer(&state.last_page, capacity)?;
            receipt.set = Some(set);
            receipt.condition = "RETPAGE".into();
            receipt.response = 32;
        }
        commit(
            service,
            run,
            request,
            Some(&current),
            Some(&next),
            &state,
            &receipt,
        )
    }
}

pub(super) fn apply_frame(
    state: &mut BmsState,
    session: &mut Session,
    frame: &ControlFrame,
) -> Result<(), HostProblem> {
    let screen_cells = u32::from(session.rows) * u32::from(session.columns);
    let cursor = frame.cursor.unwrap_or(0);
    if u32::from(cursor) >= screen_cells {
        return Err(HostProblem::Malformed);
    }
    state.cursor = cursor;
    if frame.flags & ERASE != 0 {
        session.screen.clear();
        session.map = None;
        session.mapset = None;
        session.field_values.clear();
        session.field_modified.clear();
        session.field_protection.clear();
    } else if frame.flags & ERASEAUP != 0 {
        for (name, value) in &mut session.field_values {
            if !session.field_protection.get(name).copied().unwrap_or(false) {
                value.fill(b' ');
            }
        }
        let mut image = Vec::new();
        for (name, value) in &session.field_values {
            field(&mut image, name.as_bytes())?;
            field(&mut image, value)?;
        }
        session.screen = image;
    }
    if frame.flags & FRSET != 0 {
        for modified in session.field_modified.values_mut() {
            *modified = false;
        }
    }
    if frame.flags & FREEKB != 0 {
        state.keyboard_unlocked = true;
    }
    if frame.flags & ALARM != 0 {
        state.alarm_count = state
            .alarm_count
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if frame.flags & FORMFEED != 0 {
        state.formfeed_count = state
            .formfeed_count
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if frame.flags & PRINT != 0 {
        state.print_count = state
            .print_count
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if frame.flags & DEFAULT != 0 {
        state.alternate_screen = false;
    }
    if frame.flags & ALTERNATE != 0 {
        state.alternate_screen = true;
    }
    if frame.flags & HONEOM != 0 || frame.flags & L80 != 0 {
        state.print_width = 80;
    } else if frame.flags & L64 != 0 {
        state.print_width = 64;
    } else if frame.flags & L40 != 0 {
        state.print_width = 40;
    }
    if let Some(partition) = &frame.outpartn {
        state.output_partition = Some(partition.clone());
    }
    if let Some(partition) = &frame.actpartn {
        state.active_partition = Some(partition.clone());
        state.keyboard_unlocked = true;
    }
    if let Some(msr) = frame.msr {
        state.msr_control = Some(msr);
    }
    Ok(())
}

fn frame(request: &CicsRequest, session: &Session) -> Result<ControlFrame, HostProblem> {
    let mut flags = 0u16;
    for (name, bit) in [
        ("ERASE", ERASE),
        ("ERASEAUP", ERASEAUP),
        ("FRSET", FRSET),
        ("FREEKB", FREEKB),
        ("ALARM", ALARM),
        ("PRINT", PRINT),
        ("FORMFEED", FORMFEED),
        ("DEFAULT", DEFAULT),
        ("ALTERNATE", ALTERNATE),
        ("HONEOM", HONEOM),
        ("L40", L40),
        ("L64", L64),
        ("L80", L80),
    ] {
        if has(request, name) {
            flags |= bit;
        }
    }
    let cursor = decimal(request, "CURSOR")?
        .map(|value| u16::try_from(value).map_err(|_| HostProblem::Malformed))
        .transpose()?;
    if cursor.is_some_and(|value| {
        u32::from(value) >= u32::from(session.rows) * u32::from(session.columns)
    }) {
        return Err(HostProblem::Malformed);
    }
    let name = |key: &str| {
        request
            .arguments
            .get(key)
            .map(|value| normalized_name(value.bytes(), 2))
            .transpose()
    };
    let msr = request
        .arguments
        .get("MSR")
        .map(|value| value.bytes().try_into().map_err(|_| HostProblem::Malformed))
        .transpose()?;
    Ok(ControlFrame {
        flags,
        cursor,
        outpartn: name("OUTPARTN")?,
        actpartn: name("ACTPARTN")?,
        msr,
    })
}

fn normalize_reqid(bytes: &[u8]) -> Result<String, HostProblem> {
    if bytes == b"**" {
        Ok("**".into())
    } else {
        normalized_name(bytes, 2)
    }
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const OPTIONS: &[&str] = &[
        "ACCUM",
        "FORMFEED",
        "ERASE",
        "DEFAULT",
        "ALTERNATE",
        "ERASEAUP",
        "PRINT",
        "FREEKB",
        "ALARM",
        "FRSET",
        "PAGING",
        "TERMINAL",
        "WAIT",
        "LAST",
        "HONEOM",
        "L40",
        "L64",
        "L80",
        "NOHANDLE",
    ];
    const VALUES: &[&str] = &["CURSOR", "MSR", "OUTPARTN", "ACTPARTN", "LDC", "REQID"];
    let arguments = &request.arguments;
    if request.operation != CicsOperation::SendControl
        || request.mutation.is_none()
        || arguments.contains_key("RESP2") && !arguments.contains_key("RESP")
        || has(request, "DEFAULT") && has(request, "ALTERNATE")
        || has(request, "ERASE") && has(request, "ERASEAUP")
        || [
            has(request, "L40"),
            has(request, "L64"),
            has(request, "L80"),
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
            > 1
        || [
            has(request, "TERMINAL"),
            has(request, "PAGING"),
            arguments.contains_key("SET"),
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
            > 1
        || arguments.iter().any(|(name, value)| {
            if let Some(option) = name.strip_prefix("OPTION.") {
                return !OPTIONS.contains(&option)
                    || value.schema() != "mainframe-env.cics.option@1"
                    || !value.bytes().is_empty();
            }
            if VALUES.contains(&name.as_str()) {
                return if name == "CURSOR" {
                    value.schema() != "mainframe-env.cics.decimal@1"
                } else {
                    !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    )
                };
            }
            if name == "SET.MAXLENGTH" {
                return value.schema() != "mainframe-env.cics.decimal@1";
            }
            if matches!(name.as_str(), "SET" | "RESP" | "RESP2") {
                return value.schema() != "mainframe-env.cics.argument@1";
            }
            true
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(super) fn decimal(request: &CicsRequest, name: &str) -> Result<Option<usize>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            std::str::from_utf8(value.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<usize>()
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

pub(super) fn has(request: &CicsRequest, name: &str) -> bool {
    request.arguments.contains_key(&format!("OPTION.{name}"))
}

pub(super) fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}

pub(super) fn page_pointer(page: &[u8], capacity: usize) -> Result<Vec<u8>, HostProblem> {
    let size = page
        .len()
        .checked_add(12)
        .ok_or(HostProblem::ResourceExhausted)?;
    if size > capacity {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bytes = vec![0; 12];
    bytes.extend_from_slice(page);
    Ok(bytes)
}
