use super::super::{
    CicsService, DurableContinuation, Run, argument_bytes, argument_optional, mutation_problem,
};
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    validate_context(run)?;
    let mut state = service.lock()?;
    let next_transaction =
        argument_optional(request, "TRANSID").map(|value| value.trim().to_ascii_uppercase());
    let mut commarea = argument_bytes(request, "COMMAREA").unwrap_or_default();
    if let Some(length) = argument_optional(request, "LENGTH") {
        let length = length
            .trim()
            .parse::<usize>()
            .map_err(|_| invalid_commarea_length())?;
        if length > 32_763 || length > commarea.len() {
            return Err(invalid_commarea_length());
        }
        commarea.truncate(length);
    }
    if commarea.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(transaction) = &next_transaction {
        if !matches!(transaction.len(), 1..=4)
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
    super::release_task_state(service, run)?;
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

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let invalid_argument = request
        .arguments
        .iter()
        .any(|(name, value)| match name.as_str() {
            "TRANSID" => !matches!(
                value.schema(),
                "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
                    | "mainframe-env.cics.argument@1"
            ),
            "COMMAREA" => !matches!(
                value.schema(),
                "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.argument@1"
            ),
            "LENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
            "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            }
            _ => true,
        });
    if request.mutation.is_none()
        || invalid_argument
        || request.arguments.contains_key("COMMAREA") && !request.arguments.contains_key("TRANSID")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn invalid_commarea_length() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 11,
    }
}

fn validate_context(run: &Run) -> Result<(), HostProblem> {
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
