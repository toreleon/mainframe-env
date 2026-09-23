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
        CicsOperation::Getmain64 => getmain64(service, run, request),
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

fn getmain64(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if run
        .invocation
        .bindings
        .get("cics.amode64.caller")
        .is_none_or(|value| {
            value.schema() != "mainframe-env.cics.amode64-caller@1"
                || value.bytes() != b"non-le-amode64"
        })
    {
        return Err(HostProblem::Unauthorized);
    }
    let task_data_key = run
        .invocation
        .bindings
        .get("cics.amode64.taskdatakey")
        .ok_or(HostProblem::Unauthorized)?;
    if task_data_key.schema() != "mainframe-env.cics.taskdatakey@1"
        || !matches!(task_data_key.bytes(), b"USER" | b"CICS")
    {
        return Err(HostProblem::Unauthorized);
    }
    const ALLOWED: &[&str] = &[
        "ABI64",
        "FLENGTH",
        "LOCATION",
        "OPTION.CICSDATAKEY",
        "OPTION.EXECUTABLE",
        "OPTION.NOHANDLE",
        "OPTION.NOSUSPEND",
        "OPTION.SHARED",
        "OPTION.USERDATAKEY",
        "RESP",
        "RESP2",
        "SET64",
        "SET64.LIMIT",
        "SET64.MAXLENGTH",
        "SET64.DSALIMIT",
        "SET64.DSAAVAILABLE",
    ];
    if request.mutation.is_none()
        || !request.arguments.contains_key("FLENGTH")
        || !request.arguments.contains_key("SET64")
        || !request.arguments.contains_key("SET64.LIMIT")
        || !request.arguments.contains_key("SET64.MAXLENGTH")
        || request.arguments.get("ABI64").is_none_or(|value| {
            value.schema() != "mainframe-env.cics.literal@1"
                || value.bytes() != b"mainframe-env.cics-amode64-nonle@1"
        })
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "FLENGTH" | "SET64.LIMIT" | "SET64.MAXLENGTH" | "SET64.DSALIMIT"
                    | "SET64.DSAAVAILABLE" => value.schema() != "mainframe-env.cics.decimal@1",
                    "LOCATION" | "ABI64" => value.schema() != "mainframe-env.cics.literal@1",
                    "OPTION.CICSDATAKEY" | "OPTION.EXECUTABLE" | "OPTION.NOHANDLE"
                    | "OPTION.NOSUSPEND" | "OPTION.SHARED" | "OPTION.USERDATAKEY" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "RESP" | "RESP2" | "SET64" => value.schema() != "mainframe-env.cics.argument@1",
                    _ => true,
                }
        })
        || request.arguments.contains_key("OPTION.CICSDATAKEY")
            && request.arguments.contains_key("OPTION.USERDATAKEY")
    {
        return Err(HostProblem::Malformed);
    }
    let location = match request.arguments.get("LOCATION").map(BoundedPayload::bytes) {
        None => 0,
        Some(b"LOC24") => 1,
        Some(b"LOC31") => 2,
        Some(_) => return storage64_condition(service, run, request, "INVREQ", 16, 3),
    };
    if location == 0 && request.arguments.contains_key("OPTION.EXECUTABLE") {
        return storage64_condition(service, run, request, "INVREQ", 16, 2);
    }
    // A machine-local checkpoint cannot preserve a SHARED area across a
    // different task. Keep this option closed until the durable shared arena
    // is wired to the provider state store.
    if request.arguments.contains_key("OPTION.SHARED") {
        return Err(HostProblem::Unsupported);
    }
    let length = argument_text(request, "FLENGTH")?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    i32::try_from(length).map_err(|_| HostProblem::Malformed)?;
    if !(1..=2_146_435_056).contains(&length) {
        return storage64_length_error(service, run, request);
    }
    let limit = argument_text(request, "SET64.LIMIT")?
        .parse::<u64>()
        .map_err(|_| HostProblem::Malformed)?;
    let maximum = argument_text(request, "SET64.MAXLENGTH")?
        .parse::<u64>()
        .map_err(|_| HostProblem::Malformed)?;
    if length as u64 > limit {
        return storage64_length_error(service, run, request);
    }
    let charge = ((length as u64 + 15) & !15) + 16;
    if location != 0 {
        let dsa_limit = argument_text(request, "SET64.DSALIMIT")?
            .parse::<u64>()
            .map_err(|_| HostProblem::Malformed)?;
        let dsa_available = argument_text(request, "SET64.DSAAVAILABLE")?
            .parse::<u64>()
            .map_err(|_| HostProblem::Malformed)?;
        if charge > dsa_limit {
            return storage64_length_error(service, run, request);
        }
        if charge > dsa_available {
            return storage64_condition(service, run, request, "NOSTG", 42, 2);
        }
    } else if request.arguments.contains_key("SET64.DSALIMIT")
        || request.arguments.contains_key("SET64.DSAAVAILABLE")
    {
        return Err(HostProblem::Malformed);
    }
    if charge > maximum {
        return storage64_condition(service, run, request, "NOSTG", 42, 2);
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
    let key = u8::from(
        request.arguments.contains_key("OPTION.CICSDATAKEY")
            || !request.arguments.contains_key("OPTION.USERDATAKEY")
                && task_data_key.bytes() == b"CICS",
    );
    let mut specification = vec![
        location,
        key,
        0,
        u8::from(request.arguments.contains_key("OPTION.EXECUTABLE")),
    ];
    specification.extend_from_slice(&(length as u32).to_be_bytes());
    response.outputs.insert(
        "SET64".into(),
        BoundedPayload::new(
            "mainframe-env.cics.storage64-allocation@1",
            specification,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(response)
}

fn storage64_condition(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    name: &str,
    response: i32,
    response2: i32,
) -> Result<CicsResponse, HostProblem> {
    super::condition(
        service,
        run,
        &request.condition_policy,
        HostProblem::Condition {
            name: name.into(),
            response,
            response2,
        },
    )
}

fn storage64_length_error(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let mut response = storage64_condition(service, run, request, "LENGERR", 22, 1)?;
    response.outputs.insert(
        "SET64".into(),
        BoundedPayload::new(
            "mainframe-env.cics.pointer64-null@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
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
