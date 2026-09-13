use super::super::{
    CicsService, DurableContinuation, Run, argument_bytes, argument_optional, argument_text,
    bounded, mutation_problem,
};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
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
        CicsOperation::Suspend => suspend(service, run, request),
        CicsOperation::Abend => abend(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
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
