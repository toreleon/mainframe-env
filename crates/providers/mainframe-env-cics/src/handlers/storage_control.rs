use super::super::{CicsService, Run, argument_text, bounded};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::Getmain => getmain(service, run, request),
        CicsOperation::Freemain => freemain(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn getmain(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_getmain(request)?;
    let length_name = if request.arguments.contains_key("FLENGTH") {
        "FLENGTH"
    } else {
        "LENGTH"
    };
    let length = argument_text(request, length_name)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if length_name == "FLENGTH" {
        i32::try_from(length).map_err(|_| HostProblem::Malformed)?;
    }
    if length <= 0 {
        return length_error(service, run, request);
    }
    let requested = u64::try_from(length).map_err(|_| HostProblem::Malformed)?;
    let limit = argument_text(request, "SET.LIMIT")?
        .parse::<u64>()
        .map_err(|_| HostProblem::Malformed)?;
    let limit = if length_name == "LENGTH" {
        limit.min(65_520)
    } else {
        limit
    };
    if requested > limit {
        return length_error(service, run, request);
    }
    let length = usize::try_from(requested).map_err(|_| HostProblem::Malformed)?;
    let maximum = argument_text(request, "SET.MAXLENGTH")?
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)?;
    if length > maximum {
        return super::condition(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "NOSTG".into(),
                response: 42,
                response2: 2,
            },
        );
    }
    let initial = request
        .arguments
        .get("INITIMG")
        .map(|value| value.bytes()[0])
        .unwrap_or(0);
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
    response
        .outputs
        .insert("SET".into(), bounded(vec![initial; length])?);
    Ok(response)
}

fn freemain(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let pointer = validate_freemain(request)?;
    if pointer.schema() == "mainframe-env.cics.invalid-pointer@1" {
        return super::condition(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 1,
            },
        );
    }
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
    response
        .outputs
        .insert("FREEMAIN.POINTER".into(), pointer.clone());
    Ok(response)
}

fn validate_getmain(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "FLENGTH",
        "LENGTH",
        "INITIMG",
        "OPTION.NOHANDLE",
        "OPTION.NOSUSPEND",
        "RESP",
        "RESP2",
        "SET",
        "SET.MAXLENGTH",
        "SET.LIMIT",
    ];
    let has_flength = request.arguments.contains_key("FLENGTH");
    let has_length = request.arguments.contains_key("LENGTH");
    if request.mutation.is_none()
        || has_flength == has_length
        || !request.arguments.contains_key("SET")
        || !request.arguments.contains_key("SET.MAXLENGTH")
        || !request.arguments.contains_key("SET.LIMIT")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "FLENGTH" | "LENGTH" | "SET.MAXLENGTH" | "SET.LIMIT" => {
                        value.schema() != "mainframe-env.cics.decimal@1"
                    }
                    "INITIMG" => {
                        value.schema() != "mainframe-env.cics.storage-value@1"
                            || value.bytes().len() != 1
                    }
                    "OPTION.NOHANDLE" | "OPTION.NOSUSPEND" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "RESP" | "RESP2" | "SET" => value.schema() != "mainframe-env.cics.argument@1",
                    _ => true,
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_freemain(request: &CicsRequest) -> Result<&BoundedPayload, HostProblem> {
    const ALLOWED: &[&str] = &["DATA", "DATAPOINTER", "OPTION.NOHANDLE", "RESP", "RESP2"];
    let pointer = match (
        request.arguments.get("DATA"),
        request.arguments.get("DATAPOINTER"),
    ) {
        (Some(value), None) | (None, Some(value)) => value,
        _ => return Err(HostProblem::Malformed),
    };
    if request.mutation.is_none()
        || !matches!(pointer.bytes().len(), 4 | 8)
        || !matches!(
            pointer.schema(),
            "mainframe-env.cics.allocated-pointer@1" | "mainframe-env.cics.invalid-pointer@1"
        )
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "DATA" | "DATAPOINTER" => value.schema() != pointer.schema(),
                    "OPTION.NOHANDLE" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                    _ => true,
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(pointer)
    }
}

fn length_error(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let mut response = super::condition(
        service,
        run,
        &request.condition_policy,
        HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 1,
        },
    )?;
    response.outputs.insert(
        "SET".into(),
        BoundedPayload::new(
            "mainframe-env.cics.pointer-null@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(response)
}
