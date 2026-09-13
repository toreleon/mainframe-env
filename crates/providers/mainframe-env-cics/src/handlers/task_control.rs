use super::super::{
    CicsLimits, CicsService, DurableContinuation, Reader, Run, Session, argument_bytes,
    argument_optional, argument_text, bounded, field, mutation_problem,
};
use mainframe_env_execution_api::{BoundedPayload, IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

pub(in crate::service) fn encode_session(session: &Session) -> Result<Vec<u8>, HostProblem> {
    if session.user_corr_data.len() > 64
        || session.user_corr_effect_key.is_none() && !session.user_corr_data.is_empty()
        || session.user_corr_effect_key.is_some() != session.user_corr_request_digest.is_some()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    if let Some(key) = &session.user_corr_effect_key {
        IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    let mut out = b"MECS5".to_vec();
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
    match &session.input {
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
        CicsOperation::HandleAbend => handle_abend(service, run, request),
        CicsOperation::Assign => assign(service, run, request),
        CicsOperation::Retrieve => service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            run.retrieve.clone(),
        ),
        CicsOperation::Return => return_transaction(service, run, request),
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
    super::release_task_enqueues(service, run)?;
    if request.arguments.contains_key("OPTION.CANCEL") {
        run.abend_handler = None;
    }
    service.response(
        run,
        if run.abend_handler.is_some() {
            CicsDisposition::Handler
        } else {
            CicsDisposition::Abended
        },
        "ERROR",
        27,
        0,
        run.abend_handler.clone(),
        None,
        argument_bytes(request, "ABCODE").unwrap_or_default(),
    )
}

fn handle_condition(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let (condition, label) = request
        .arguments
        .iter()
        .find(|(name, _)| !name.starts_with("OPTION."))
        .map(|(name, value)| {
            (
                name.to_ascii_uppercase(),
                String::from_utf8_lossy(value.bytes()).into_owned(),
            )
        })
        .ok_or(HostProblem::Malformed)?;
    run.handlers.insert(condition, label);
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
    if request.arguments.contains_key("OPTION.CANCEL") {
        run.abend_handler = None;
    } else {
        run.abend_handler = Some(argument_text(request, "LABEL")?);
    }
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

fn assign(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
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
    for (name, value) in [
        ("APPLID", run.applid.as_bytes()),
        ("SYSID", run.sysid.as_bytes()),
        ("TRANSID", run.transaction.as_bytes()),
        (
            "PRINCIPAL",
            run.invocation.principal.id().as_str().as_bytes(),
        ),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), bounded(value.to_vec())?);
        }
    }
    Ok(response)
}

fn return_transaction(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let mut state = service.lock()?;
    let next_transaction = argument_optional(request, "TRANSID")
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty());
    let commarea = argument_bytes(request, "COMMAREA").unwrap_or_default();
    if commarea.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(transaction) = &next_transaction {
        if transaction.len() > 16
            || !transaction
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(HostProblem::Malformed);
        }
        let effect_key = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?
            .idempotency_key
            .as_str()
            .to_string();
        let current = state.continuations.get(&run.session).cloned();
        if let Some(current) = &current
            && current.effect_key == effect_key
        {
            if current.transaction != *transaction || current.commarea != commarea {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else {
            let version = current.as_ref().map_or(Ok(1), |value| {
                value
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
            let next = DurableContinuation {
                transaction: transaction.clone(),
                commarea: commarea.clone(),
                claimed_by: None,
                effect_key,
                version,
            };
            service
                .persist_continuation(
                    &run.session,
                    &next,
                    current.as_ref().map(|value| value.version),
                )
                .map_err(mutation_problem)?;
            state.continuations.insert(run.session.clone(), next);
        }
    } else if let Some(current) = state.continuations.get(&run.session).cloned()
        && current.claimed_by.as_deref() == Some(run.invocation.run_unit_id.as_str())
    {
        service
            .store
            .delete_provider_state("cics-continuation", &run.session, current.version)
            .map_err(super::super::store_error)
            .map_err(mutation_problem)?;
        state.continuations.remove(&run.session);
    }
    super::release_task_enqueues(service, run)?;
    service.response(
        run,
        CicsDisposition::Returned,
        "NORMAL",
        0,
        0,
        None,
        next_transaction,
        commarea,
    )
}
