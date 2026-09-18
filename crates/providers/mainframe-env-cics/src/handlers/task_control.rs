use super::super::{
    CicsLimits, CicsService, DatasetUndo, Reader, Run, Session, argument_bytes, argument_text,
    field,
};
use super::handle_state::{
    AbendExit, AbendRecord, HandleFrame, HandleState, MAX_HANDLE_STACK_DEPTH, encode_handle_state,
    persist_handle_state,
};
use crate::generated::{CICS_AID_NAMES, CICS_CONDITION_NAMES};
use mainframe_env_execution_api::{BoundedPayload, IdempotencyKey, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, CicsResponse,
    HostProblem, HostRequest, canonical_request_digest,
};
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::service) struct RunSeed {
    pub(in crate::service) originating_task: String,
    pub(in crate::service) retrieve: Vec<u8>,
    pub(in crate::service) undo: Vec<DatasetUndo>,
    pub(in crate::service) undo_version: Option<u64>,
    pub(in crate::service) handle_state: HandleState,
}

pub(in crate::service) fn new_run(
    invocation: Invocation,
    session: &str,
    transaction: &str,
    applid: &str,
    sysid: &str,
) -> Run {
    let originating_task = invocation.run_unit_id.as_str().to_string();
    let retrieve = invocation
        .bindings
        .get("cics.retrieve")
        .map(|value| value.bytes().to_vec())
        .unwrap_or_default();
    new_run_with_state(
        invocation,
        session,
        transaction,
        applid,
        sysid,
        RunSeed {
            originating_task,
            retrieve,
            undo: Vec::new(),
            undo_version: None,
            handle_state: HandleState::default(),
        },
    )
}

pub(in crate::service) fn new_run_with_state(
    invocation: Invocation,
    session: &str,
    transaction: &str,
    applid: &str,
    sysid: &str,
    seed: RunSeed,
) -> Run {
    let current_program = super::CurrentProgramFrame {
        current: super::task_context::current_program(&invocation),
        parent_execution_id: invocation.parent_execution_id.clone(),
        initial_entry: false,
    };
    let HandleState {
        handlers,
        aid_handlers,
        ignored_conditions,
        abend_handler,
        cancelled_abend_handler,
        stack,
        latest_abend,
    } = seed.handle_state;
    Run {
        invocation,
        current_program,
        session: session.into(),
        transaction: transaction.to_ascii_uppercase(),
        applid: applid.to_ascii_uppercase(),
        sysid: sysid.to_ascii_uppercase(),
        originating_task: seed.originating_task,
        host_sequence: 0,
        outer_effect_key: None,
        handlers,
        aid_handlers,
        ignored_conditions,
        abend_handler,
        cancelled_abend_handler,
        handle_stack: stack,
        latest_abend,
        retrieve: seed.retrieve,
        current_records: BTreeMap::new(),
        current_record_values: BTreeMap::new(),
        undo: seed.undo,
        undo_version: seed.undo_version,
        browses: BTreeMap::new(),
        trace: Vec::new(),
    }
}

pub(in crate::service) fn encode_session(session: &Session) -> Result<Vec<u8>, HostProblem> {
    if session.user_corr_data.len() > 64
        || session.user_corr_effect_key.is_none() && !session.user_corr_data.is_empty()
        || session.user_corr_effect_key.is_some() != session.user_corr_request_digest.is_some()
        || session.input.message_length > 32_767
        || session.input.terminal_id.as_ref().is_some_and(|value| {
            value.len() != 4
                || !value.as_bytes()[0].is_ascii_uppercase()
                || !value.bytes().all(|byte| {
                    byte.is_ascii_uppercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'$' | b'@' | b'#')
                })
        })
        || session.input.payload.as_ref().is_some_and(|payload| {
            usize::try_from(session.input.message_length).ok() != Some(payload.len())
        })
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    if let Some(key) = &session.user_corr_effect_key {
        IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    let mut out = b"MECSB".to_vec();
    out.extend_from_slice(&session.rows.to_be_bytes());
    out.extend_from_slice(&session.columns.to_be_bytes());
    field(&mut out, session.principal.as_bytes())?;
    field(&mut out, session.transaction.as_bytes())?;
    field(&mut out, session.run_unit.as_bytes())?;
    field(&mut out, session.csrf_sha256.as_bytes())?;
    out.extend_from_slice(&session.idle_timeout_ticks.to_be_bytes());
    out.extend_from_slice(&session.expires_at_tick.to_be_bytes());
    out.push(u8::from(session.connected));
    out.push(session.aid);
    out.push(u8::from(session.suspended));
    field(&mut out, session.mapset.as_deref().unwrap_or("").as_bytes())?;
    field(&mut out, session.map.as_deref().unwrap_or("").as_bytes())?;
    out.extend_from_slice(
        &u32::try_from(session.field_protection.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, protected) in &session.field_protection {
        field(&mut out, name.as_bytes())?;
        out.push(u8::from(*protected));
    }
    out.extend_from_slice(
        &u32::try_from(session.field_modified.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, modified) in &session.field_modified {
        field(&mut out, name.as_bytes())?;
        out.push(u8::from(*modified));
    }
    out.extend_from_slice(
        &u32::try_from(session.field_values.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, value) in &session.field_values {
        field(&mut out, name.as_bytes())?;
        field(&mut out, value)?;
    }
    field(&mut out, &session.screen)?;
    match &session.input.payload {
        Some(input) => {
            out.push(1);
            field(&mut out, input)?;
        }
        None => out.push(0),
    }
    field(&mut out, &session.user_corr_data)?;
    field(
        &mut out,
        session
            .user_corr_effect_key
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    )?;
    match session.user_corr_request_digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(&digest);
        }
        None => out.push(0),
    }
    encode_handle_state(&mut out, &session.handle_state)?;
    out.extend_from_slice(&session.input.message_length.to_be_bytes());
    field(
        &mut out,
        session
            .input
            .terminal_id
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    )?;
    Ok(out)
}

pub(in crate::service) fn decode_session_flags(
    reader: &mut Reader<'_>,
    limits: CicsLimits,
) -> Result<BTreeMap<String, bool>, HostProblem> {
    let count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if count > limits.max_fields {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut values = BTreeMap::new();
    for _ in 0..count {
        let name =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let value = match reader.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        if values.insert(name, value).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(values)
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::AddressSet => address_set(service, run, request),
        CicsOperation::ChangeTask => change_task(service, run, request),
        CicsOperation::Deq | CicsOperation::Enq => {
            super::task_enqueue::invoke(service, run, request, retention_tick)
        }
        CicsOperation::HandleCondition => handle_condition(service, run, request),
        CicsOperation::HandleAid => handle_aid(service, run, request),
        CicsOperation::HandleAbend => handle_abend(service, run, request),
        CicsOperation::IgnoreCondition => ignore_condition(service, run, request),
        CicsOperation::PopHandle => pop_handle(service, run, request),
        CicsOperation::PushHandle => push_handle(service, run, request),
        CicsOperation::Assign => super::task_context::assign(service, run, request),
        CicsOperation::Retrieve => super::interval_control::invoke(service, run, request),
        CicsOperation::Return => super::task_return::invoke(service, run, request),
        CicsOperation::SetAssociationUserCorrData => {
            set_association_user_corr_data(service, run, request)
        }
        CicsOperation::Suspend => suspend(service, run, request),
        CicsOperation::Abend => abend(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn address_set(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_address_set_request(request)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn validate_address_set_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
        "SET.ADDRESS",
        "SET.POINTER",
        "USING.ADDRESS",
        "USING.POINTER",
    ];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
    }) {
        return Err(HostProblem::Malformed);
    }
    let has = |name| request.arguments.contains_key(name);
    let pointer_from_data =
        has("SET.POINTER") && has("USING.ADDRESS") && !has("SET.ADDRESS") && !has("USING.POINTER");
    let data_from_pointer =
        has("SET.ADDRESS") && has("USING.POINTER") && !has("SET.POINTER") && !has("USING.ADDRESS");
    if !pointer_from_data && !data_from_pointer {
        return Err(HostProblem::Malformed);
    }
    for name in ["SET.ADDRESS", "SET.POINTER"] {
        if request
            .arguments
            .get(name)
            .is_some_and(|value| value.schema() != "mainframe-env.cics.storage-target@1")
        {
            return Err(HostProblem::Malformed);
        }
    }
    if request
        .arguments
        .get("USING.ADDRESS")
        .is_some_and(|value| value.schema() != "mainframe-env.cics.storage-identity@1")
    {
        return Err(HostProblem::Malformed);
    }
    if request.arguments.get("USING.POINTER").is_some_and(|value| {
        value.schema() != "mainframe-env.cics.storage-value@1"
            || !matches!(value.bytes().len(), 4 | 8)
    }) {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn change_task(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_scheduling_request(request, true)?;
    let Some(priority) = request.arguments.get("PRIORITY") else {
        return service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        );
    };
    let priority = std::str::from_utf8(priority.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if priority == -1 {
        return service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    let priority = u8::try_from(priority).map_err(|_| HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 1,
    })?;
    run.invocation.priority = priority;
    let mut response = service.response(
        run,
        CicsDisposition::Suspended,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    response.outputs.insert(
        "TASK.PRIORITY".into(),
        BoundedPayload::new(
            "mainframe-env.cics.decimal@1",
            priority.to_string().into_bytes(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(response)
}

fn suspend(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_scheduling_request(request, false)?;
    service.response(
        run,
        CicsDisposition::Suspended,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn set_association_user_corr_data(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_task_association_request(request)?;
    service
        .authorize(
            run,
            "FACILITY",
            "CICS.COMMAND.SET.ASSOCIATION.USERCORRDATA",
            AccessIntent::Alter,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 100,
            },
            other => other,
        })?;
    if run.originating_task != run.invocation.run_unit_id.as_str() {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 1,
        });
    }
    let input = request
        .arguments
        .get("USERCORRDATA")
        .ok_or(HostProblem::Malformed)?;
    let value = input.bytes()[..input.bytes().len().min(64)].to_vec();
    let effect_key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let mut state = service.lock()?;
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    if current.user_corr_effect_key.as_deref() == Some(effect_key) {
        if current.user_corr_request_digest != Some(request_digest) {
            return Err(HostProblem::IdempotencyConflict);
        }
    } else {
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.user_corr_data = value;
        next.user_corr_effect_key = Some(effect_key.into());
        next.user_corr_request_digest = Some(request_digest);
        service.persist_session(&run.session, &next, Some(current.version))?;
        state.sessions.insert(run.session.clone(), next);
    }
    drop(state);
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn validate_task_association_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = ["OPTION.NOHANDLE", "RESP", "RESP2", "USERCORRDATA"];
    let value = request
        .arguments
        .get("USERCORRDATA")
        .ok_or(HostProblem::Malformed)?;
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
    }) || !matches!(
        value.schema(),
        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
    ) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_scheduling_request(
    request: &CicsRequest,
    accepts_priority: bool,
) -> Result<(), HostProblem> {
    let allowed = if accepts_priority {
        &["OPTION.NOHANDLE", "PRIORITY", "RESP", "RESP2"][..]
    } else {
        &["OPTION.NOHANDLE", "RESP", "RESP2"][..]
    };
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
    }) || request
        .arguments
        .get("PRIORITY")
        .is_some_and(|value| value.schema() != "mainframe-env.cics.decimal@1")
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn abend(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_abend_request(request)?;
    super::interval_control::discard_protected_starts(service, run)?;
    super::release_task_state(service, run)?;
    let code = argument_bytes(request, "ABCODE").unwrap_or_default();
    let dump_requested =
        !request.arguments.contains_key("OPTION.NODUMP") && valid_abend_code(&code);
    let previous = HandleState::from_run(run);
    let exit = if request.arguments.contains_key("OPTION.CANCEL") {
        run.abend_handler = None;
        run.cancelled_abend_handler = None;
        None
    } else if let Some(exit) = run.abend_handler.take() {
        run.cancelled_abend_handler = Some(exit.clone());
        Some(exit)
    } else {
        None
    };
    let original_code = run
        .latest_abend
        .as_ref()
        .map_or_else(|| code.clone(), |record| record.original_code.clone());
    run.latest_abend = valid_abend_code(&code).then(|| AbendRecord {
        code: code.clone(),
        original_code,
        dump_requested,
        program: run.current_program.current.clone(),
    });
    if HandleState::from_run(run) != previous {
        persist_handle_state(service, run, previous)?;
    }
    let (disposition, target) = match exit {
        Some(AbendExit::Label(target)) => (CicsDisposition::Handler, Some(target)),
        Some(AbendExit::Program(target)) => (CicsDisposition::Transfer, Some(target)),
        None => (CicsDisposition::Abended, None),
    };
    let dump = if dump_requested {
        b"requested".as_slice()
    } else {
        b"suppressed".as_slice()
    };
    let payload = if disposition == CicsDisposition::Transfer {
        run.retrieve.clone()
    } else {
        code
    };
    let mut response = service.response(run, disposition, "ERROR", 27, 0, target, None, payload)?;
    if disposition == CicsDisposition::Abended {
        response.outputs.insert(
            "ABEND.DUMP".into(),
            BoundedPayload::new(
                "mainframe-env.cics.abend-dump@1",
                dump.to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
}

fn validate_abend_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "ABCODE",
        "OPTION.CANCEL",
        "OPTION.NODUMP",
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
    ];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
            || name == "ABCODE"
                && !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                        | "mainframe-env.cics.argument@1"
                )
    }) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn valid_abend_code(code: &[u8]) -> bool {
    matches!(code.len(), 1..=4)
        && !code[0].eq_ignore_ascii_case(&b'A')
        && code.iter().all(|byte| byte.is_ascii_graphic())
}

fn handle_condition(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let handlers = condition_handlers(request)?;
    let previous = HandleState::from_run(run);
    for (condition, label) in handlers {
        run.ignored_conditions.remove(&condition);
        if label.is_empty() {
            run.handlers.remove(&condition);
        } else {
            run.handlers.insert(condition, label);
        }
    }
    persist_handle_state(service, run, previous)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn handle_aid(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_handle_aid_context(run)?;
    let allowed = ["AIDS", "OPTION.NOHANDLE", "RESP", "RESP2"];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
    }) {
        return Err(HostProblem::Malformed);
    }
    let value = request
        .arguments
        .get("AIDS")
        .ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.aid-handlers@1" {
        return Err(HostProblem::Malformed);
    }
    let handlers = parse_aid_handlers(value.bytes())?;
    let previous = HandleState::from_run(run);
    for (name, label) in handlers {
        run.aid_handlers.insert(name, label);
    }
    persist_handle_state(service, run, previous)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn validate_handle_aid_context(run: &Run) -> Result<(), HostProblem> {
    let Some(context) = run.invocation.bindings.get("cics.execution-context") else {
        return Ok(());
    };
    if context.schema() != "mainframe-env.cics.execution-context@1" {
        return Err(HostProblem::Malformed);
    }
    match context.bytes() {
        b"local" => Ok(()),
        b"dpl-synconreturn" | b"dpl-without-synconreturn" | b"dpl-executionset-subset" => {
            Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 200,
            })
        }
        _ => Err(HostProblem::Malformed),
    }
}

fn parse_aid_handlers(bytes: &[u8]) -> Result<Vec<(String, String)>, HostProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostProblem::Malformed)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = BTreeSet::new();
    let mut handlers: Vec<(String, String)> = Vec::new();
    for entry in text.split('\n') {
        let (name, label) = entry.split_once('\t').ok_or(HostProblem::Malformed)?;
        if CICS_AID_NAMES.binary_search(&name).is_err()
            || !names.insert(name)
            || !valid_condition_label(label)
        {
            return Err(HostProblem::Malformed);
        }
        handlers.push((name.into(), label.into()));
    }
    if handlers.len() <= 16
        && handlers
            .windows(2)
            .all(|pair| pair[0].0.as_str() < pair[1].0.as_str())
    {
        Ok(handlers)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn condition_handlers(request: &CicsRequest) -> Result<Vec<(String, String)>, HostProblem> {
    if let Some(value) = request.arguments.get("CONDITIONS") {
        let allowed = ["CONDITIONS", "OPTION.NOHANDLE", "RESP", "RESP2"];
        if request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || name.starts_with("OPTION.") && !value.bytes().is_empty()
        }) || value.schema() != "mainframe-env.cics.condition-handlers@1"
        {
            return Err(HostProblem::Malformed);
        }
        return parse_condition_handlers(value.bytes());
    }

    let mut names = BTreeSet::new();
    let mut handlers: Vec<(String, String)> = Vec::new();
    for (name, value) in &request.arguments {
        if name.starts_with("OPTION.") {
            if name != "OPTION.NOHANDLE" || !value.bytes().is_empty() {
                return Err(HostProblem::Malformed);
            }
            continue;
        }
        if matches!(name.as_str(), "RESP" | "RESP2") {
            continue;
        }
        let condition = name.to_ascii_uppercase();
        if CICS_CONDITION_NAMES
            .binary_search(&condition.as_str())
            .is_err()
            || !names.insert(condition.clone())
            || value.schema() != "mainframe-env.cics.argument@1"
        {
            return Err(HostProblem::Malformed);
        }
        let label = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
        if !valid_condition_label(label) {
            return Err(HostProblem::Malformed);
        }
        handlers.push((condition, label.into()));
    }
    if matches!(handlers.len(), 1..=16) {
        Ok(handlers)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn parse_condition_handlers(bytes: &[u8]) -> Result<Vec<(String, String)>, HostProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostProblem::Malformed)?;
    let mut names = BTreeSet::new();
    let mut handlers: Vec<(String, String)> = Vec::new();
    for entry in text.split('\n') {
        let (name, label) = entry.split_once('\t').ok_or(HostProblem::Malformed)?;
        if CICS_CONDITION_NAMES.binary_search(&name).is_err()
            || !names.insert(name)
            || !valid_condition_label(label)
        {
            return Err(HostProblem::Malformed);
        }
        handlers.push((name.into(), label.into()));
    }
    if matches!(handlers.len(), 1..=16)
        && handlers
            .windows(2)
            .all(|pair| pair[0].0.as_str() < pair[1].0.as_str())
    {
        Ok(handlers)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn valid_condition_label(label: &str) -> bool {
    label.len() <= InvocationLimits::default().max_identity_bytes
        && (label.is_empty()
            || label
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-'))
}

fn ignore_condition(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let allowed = ["CONDITIONS", "OPTION.NOHANDLE", "RESP", "RESP2"];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
    }) {
        return Err(HostProblem::Malformed);
    }
    let value = request
        .arguments
        .get("CONDITIONS")
        .ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.condition-list@1" {
        return Err(HostProblem::Malformed);
    }
    let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
    let names = text.split('\n').collect::<Vec<_>>();
    let unique = names.iter().copied().collect::<BTreeSet<_>>();
    if !matches!(names.len(), 1..=16)
        || unique.len() != names.len()
        || names
            .iter()
            .any(|name| CICS_CONDITION_NAMES.binary_search(name).is_err())
    {
        return Err(HostProblem::Malformed);
    }
    let previous = HandleState::from_run(run);
    for name in names {
        run.handlers.remove(name);
        run.ignored_conditions.insert(name.into());
    }
    persist_handle_state(service, run, previous)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn handle_abend(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let action = handle_abend_action(request)?;
    if let AbendHandlerAction::Exit(AbendExit::Program(program)) = &action {
        service.authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{program}"),
            AccessIntent::Execute,
        )?;
        if !service.lock()?.programs.contains(program) {
            return Err(HostProblem::Condition {
                name: "PGMIDERR".into(),
                response: 27,
                response2: 1,
            });
        }
    }
    let previous = HandleState::from_run(run);
    match action {
        AbendHandlerAction::Cancel => {
            if let Some(handler) = run.abend_handler.take() {
                run.cancelled_abend_handler = Some(handler);
            }
        }
        AbendHandlerAction::Exit(exit) => {
            run.abend_handler = Some(exit);
            run.cancelled_abend_handler = None;
        }
        AbendHandlerAction::Reset => {
            if let Some(handler) = run.cancelled_abend_handler.take() {
                run.abend_handler = Some(handler);
            }
        }
    }
    persist_handle_state(service, run, previous)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

enum AbendHandlerAction {
    Cancel,
    Exit(AbendExit),
    Reset,
}

fn handle_abend_action(request: &CicsRequest) -> Result<AbendHandlerAction, HostProblem> {
    let allowed = [
        "LABEL",
        "OPTION.CANCEL",
        "OPTION.NOHANDLE",
        "OPTION.RESET",
        "PROGRAM",
        "RESP",
        "RESP2",
    ];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
            || name == "LABEL"
                && !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.argument@1"
                )
            || name == "PROGRAM"
                && !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                        | "mainframe-env.cics.argument@1"
                )
    }) {
        return Err(HostProblem::Malformed);
    }
    let cancel = request.arguments.contains_key("OPTION.CANCEL");
    let label = request.arguments.contains_key("LABEL");
    let program = request.arguments.contains_key("PROGRAM");
    let reset = request.arguments.contains_key("OPTION.RESET");
    if usize::from(cancel) + usize::from(label) + usize::from(program) + usize::from(reset) > 1 {
        return Err(HostProblem::Malformed);
    }
    if label {
        let label = argument_text(request, "LABEL")?;
        if !valid_condition_label(&label) {
            return Err(HostProblem::Malformed);
        }
        Ok(AbendHandlerAction::Exit(AbendExit::Label(label)))
    } else if program {
        let program = argument_text(request, "PROGRAM")?
            .trim()
            .to_ascii_uppercase();
        if !matches!(program.len(), 1..=8)
            || !program.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(HostProblem::Malformed);
        }
        Ok(AbendHandlerAction::Exit(AbendExit::Program(program)))
    } else if reset {
        Ok(AbendHandlerAction::Reset)
    } else {
        // IBM defines CANCEL as the default when no action option is present.
        Ok(AbendHandlerAction::Cancel)
    }
}

fn push_handle(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_handle_stack_request(request)?;
    if run.handle_stack.len() >= MAX_HANDLE_STACK_DEPTH {
        return Err(HostProblem::ResourceExhausted);
    }
    let previous = HandleState::from_run(run);
    run.handle_stack.push(HandleFrame {
        handlers: std::mem::take(&mut run.handlers),
        aid_handlers: std::mem::take(&mut run.aid_handlers),
        ignored_conditions: std::mem::take(&mut run.ignored_conditions),
        abend_handler: run.abend_handler.take(),
        cancelled_abend_handler: run.cancelled_abend_handler.take(),
    });
    persist_handle_state(service, run, previous)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn pop_handle(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_handle_stack_request(request)?;
    let previous = HandleState::from_run(run);
    let Some(frame) = run.handle_stack.pop() else {
        return pop_handle_invreq(service, run, request);
    };
    run.handlers = frame.handlers;
    run.aid_handlers = frame.aid_handlers;
    run.ignored_conditions = frame.ignored_conditions;
    run.abend_handler = frame.abend_handler;
    run.cancelled_abend_handler = frame.cancelled_abend_handler;
    persist_handle_state(service, run, previous)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn validate_handle_stack_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = ["OPTION.NOHANDLE", "RESP", "RESP2"];
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || name.starts_with("OPTION.") && !value.bytes().is_empty()
    }) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn pop_handle_invreq(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let previous = HandleState::from_run(run);
    let (disposition, target, payload) = match &request.condition_policy {
        CicsConditionPolicy::NoHandle | CicsConditionPolicy::Respond { .. } => {
            (CicsDisposition::Complete, None, Vec::new())
        }
        CicsConditionPolicy::Default if run.ignored_conditions.contains("INVREQ") => {
            (CicsDisposition::Ignored, None, Vec::new())
        }
        CicsConditionPolicy::Default if run.handlers.contains_key("INVREQ") => (
            CicsDisposition::Handler,
            run.handlers.get("INVREQ").cloned(),
            Vec::new(),
        ),
        CicsConditionPolicy::Default if run.ignored_conditions.contains("ERROR") => {
            (CicsDisposition::Ignored, None, Vec::new())
        }
        CicsConditionPolicy::Default if run.handlers.contains_key("ERROR") => (
            CicsDisposition::Handler,
            run.handlers.get("ERROR").cloned(),
            Vec::new(),
        ),
        CicsConditionPolicy::Default if run.abend_handler.is_some() => {
            let exit = run
                .abend_handler
                .take()
                .ok_or(HostProblem::InfrastructureFailure)?;
            run.cancelled_abend_handler = Some(exit.clone());
            match exit {
                AbendExit::Label(target) => (CicsDisposition::Handler, Some(target), Vec::new()),
                AbendExit::Program(target) => (
                    CicsDisposition::Transfer,
                    Some(target),
                    run.retrieve.clone(),
                ),
            }
        }
        CicsConditionPolicy::Default => (CicsDisposition::Abended, None, Vec::new()),
    };
    if HandleState::from_run(run) != previous {
        persist_handle_state(service, run, previous)?;
    }
    service.response(run, disposition, "INVREQ", 16, 0, target, None, payload)
}
