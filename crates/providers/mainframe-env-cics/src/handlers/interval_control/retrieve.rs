use super::super::super::{CicsService, Run};
use super::{IntervalConsumeRequest, consume_next, optional_decimal};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.is_empty() && !run.retrieve.is_empty() {
        return service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            run.retrieve.clone(),
        );
    }
    validate_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let now_tick = service.durable_tick()?;
    let max_data_length = optional_decimal(request, "SET.MAXLENGTH")?
        .map(|value| usize::try_from(value).map_err(|_| HostProblem::Malformed))
        .transpose()?;
    let into_capacity = optional_decimal(request, "INTO.MAXLENGTH")?
        .map(|value| usize::try_from(value).map_err(|_| HostProblem::Malformed))
        .transpose()?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let record = {
        let mut state = service.lock()?;
        let terminal = state
            .sessions
            .get(&run.session)
            .and_then(|session| session.input.terminal_id.clone());
        consume_next(
            service.store.as_ref(),
            &mut state.interval_records,
            IntervalConsumeRequest {
                transaction: &run.transaction,
                terminal: terminal.as_deref(),
                now_tick,
                effect_key: mutation.idempotency_key.as_str(),
                request_digest: digest,
                return_transaction: request.arguments.contains_key("RTRANSID"),
                return_terminal: request.arguments.contains_key("RTERMID"),
                queue: request.arguments.contains_key("QUEUE"),
                max_data_length,
            },
            service.limits,
        )?
    };
    let Some(record) = record else {
        if request.arguments.contains_key("OPTION.WAIT") {
            return service.response(
                run,
                CicsDisposition::Suspended,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            );
        }
        return super::super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "ENDDATA".into(),
                response: 29,
                response2: 0,
            },
        );
    };
    if record.data.is_empty()
        && record.return_transaction.is_none()
        && record.return_terminal.is_none()
        && record.queue.is_none()
    {
        return super::super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "ENDDATA".into(),
                response: 29,
                response2: 0,
            },
        );
    }
    let actual = record.data.len();
    let maximum = if request.arguments.contains_key("INTO") {
        let requested = optional_decimal(request, "LENGTH")?
            .map(|value| usize::try_from(value.max(0)).unwrap_or(0))
            .or(into_capacity)
            .ok_or(HostProblem::Malformed)?;
        requested.min(into_capacity.unwrap_or(requested))
    } else {
        actual
    };
    let returned = record.data[..actual.min(maximum)].to_vec();
    let mut response = if maximum < actual {
        super::super::condition::respond(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 0,
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
            returned.clone(),
        )?
    };
    let returned_payload = BoundedPayload::new(
        "mainframe-env.cics.payload@1",
        returned.clone(),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)?;
    response.payload = returned_payload.clone();
    if request.arguments.contains_key("INTO") {
        response
            .outputs
            .insert("INTO".into(), returned_payload.clone());
    }
    if request.arguments.contains_key("SET") {
        response.outputs.insert("SET".into(), returned_payload);
    }
    if request.arguments.contains_key("LENGTH") {
        response.outputs.insert(
            "LENGTH".into(),
            BoundedPayload::new(
                "mainframe-env.cics.decimal@1",
                actual.to_string().into_bytes(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    response.outputs.insert(
        "EIBFMH".into(),
        BoundedPayload::new(
            "mainframe-env.cics.eib-fmh@1",
            vec![if record.fmh { 0xff } else { 0x00 }],
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    for (name, value) in [
        ("RTRANSID", record.return_transaction.as_deref()),
        ("RTERMID", record.return_terminal.as_deref()),
        ("QUEUE", record.queue.as_deref()),
    ] {
        if request.arguments.contains_key(name) {
            response.outputs.insert(
                name.into(),
                BoundedPayload::new(
                    "mainframe-env.cics.payload@1",
                    value
                        .ok_or(HostProblem::InfrastructureFailure)?
                        .as_bytes()
                        .to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
    }
    Ok(response)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "INTO",
        "INTO.MAXLENGTH",
        "LENGTH",
        "OPTION.NOHANDLE",
        "OPTION.WAIT",
        "QUEUE",
        "RESP",
        "RESP2",
        "RTERMID",
        "RTRANSID",
        "SET",
        "SET.MAXLENGTH",
    ];
    let into_form = request.arguments.contains_key("INTO");
    let set_form = request.arguments.contains_key("SET");
    if into_form == set_form
        || (set_form && !request.arguments.contains_key("LENGTH"))
        || (into_form
            && !request.arguments.contains_key("LENGTH")
            && !request.arguments.contains_key("INTO.MAXLENGTH"))
        || set_form != request.arguments.contains_key("SET.MAXLENGTH")
        || (!into_form && request.arguments.contains_key("INTO.MAXLENGTH"))
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || if name == "LENGTH" {
                    value.schema()
                        != if into_form {
                            "mainframe-env.cics.decimal@1"
                        } else {
                            "mainframe-env.cics.argument@1"
                        }
                } else if matches!(name.as_str(), "SET.MAXLENGTH" | "INTO.MAXLENGTH") {
                    value.schema() != "mainframe-env.cics.decimal@1"
                } else if matches!(name.as_str(), "OPTION.NOHANDLE" | "OPTION.WAIT") {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                } else {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}
