use super::super::super::{CicsService, Run, argument_bytes, bounded};
use super::{
    CicsApplicationEntryDefinition, CicsJavaStatus, condition, decimal_usize,
    normalize_application_name, validate_program_artifact,
};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, HostResult,
    ProgramLinkSelection, ProgramName, ProgramRequest,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let application = application_argument(request, "APPLICATION")?;
    let operation = application_argument(request, "OPERATION")?;
    let platform = match request.arguments.get("PLATFORM") {
        Some(_) => application_argument(request, "PLATFORM")?,
        // The topic description says APPNOTFOUND, but the command condition
        // table assigns this exact case to INVREQ RESP2 1. Preserve the table's
        // explicit response mapping.
        None => current_platform(run).ok_or_else(|| condition("INVREQ", 16, 1))?,
    };
    let major = optional_version(request, "MAJORVERSION")?;
    let minor = optional_version(request, "MINORVERSION")?;
    if major.is_some() != minor.is_some() {
        return Err(HostProblem::Malformed);
    }
    let exact = request.arguments.contains_key("OPTION.EXACTMATCH");
    let minimum = request.arguments.contains_key("OPTION.MINIMUM");
    if exact && minimum || (exact || minimum) && major.is_none() {
        return Err(HostProblem::Malformed);
    }
    let (entry, program) = {
        let state = service.lock()?;
        let candidates = state.application_entries.iter().filter(|entry| {
            entry.available
                && entry.application == application
                && entry.platform == platform
                && entry.operation == operation
        });
        let selected = select_entry(candidates, major, minor, minimum).ok_or_else(|| {
            condition(
                "APPNOTFOUND",
                127,
                if exact {
                    1
                } else if minimum {
                    2
                } else {
                    3
                },
            )
        })?;
        let program = state
            .program_definitions
            .get(&selected.program)
            .and_then(|generations| generations.get(&selected.program_generation))
            .cloned()
            .ok_or_else(|| condition("PGMIDERR", 27, 2))?;
        (selected.clone(), program)
    };
    if program.artifact != entry.program_artifact {
        return Err(condition("PGMIDERR", 27, 2));
    }
    if !program.enabled {
        return Err(condition("PGMIDERR", 27, 1));
    }
    if program.remote {
        return Err(condition("PGMIDERR", 27, 2));
    }
    match program.java_status {
        CicsJavaStatus::ClassUnavailable => return Err(condition("INVREQ", 16, 2)),
        CicsJavaStatus::ServerNotFound => return Err(condition("INVREQ", 16, 3)),
        CicsJavaStatus::ServerDisabled => return Err(condition("INVREQ", 16, 4)),
        CicsJavaStatus::NotJava | CicsJavaStatus::Available => {}
    }
    validate_program_artifact(
        service
            .artifacts
            .get()
            .ok_or_else(|| condition("PGMIDERR", 27, 2))?
            .as_ref(),
        &program,
    )
    .map_err(|_| condition("PGMIDERR", 27, 2))?;
    let mut payload = payload(request)?;
    if let Some(raw) = request.arguments.get("LENGTH") {
        let length = decimal_usize(raw).filter(|length| matches!(*length, 1..=24_576));
        let Some(length) = length else {
            return Err(condition("LENGERR", 22, 11));
        };
        if !request.arguments.contains_key("COMMAREA") {
            return Err(condition("LENGERR", 22, 26));
        }
        if length > payload.bytes().len() {
            return Err(condition("LENGERR", 22, 11));
        }
        payload = bounded(payload.bytes()[..length].to_vec())?;
    }
    service
        .authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{}", program.name),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => condition("NOTAUTH", 70, 101),
            problem => problem,
        })?;
    let result = service.nested(
        run,
        HostRequest::Program(ProgramRequest::Link {
            program: ProgramName::new(&program.name, 128).map_err(|_| HostProblem::Malformed)?,
            payload,
            selection: Some(ProgramLinkSelection {
                artifact: entry.program_artifact.clone(),
                generation: entry.program_generation,
                content_identity: entry.application_identity.clone(),
            }),
        }),
    );
    let returned = match result {
        Ok(HostResult::Program(payload)) => payload,
        Err(HostProblem::NotFound) => return Err(condition("PGMIDERR", 27, 2)),
        Err(problem) => return Err(problem),
        Ok(_) => return Err(HostProblem::ProviderFailure),
    };
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        Some(program.name),
        None,
        returned.bytes().to_vec(),
    )?;
    if request.arguments.contains_key("COMMAREA") {
        response
            .outputs
            .insert("COMMAREA".into(), bounded(returned.bytes().to_vec())?);
    }
    response.outputs.insert(
        "APPLICATION.IDENTITY".into(),
        bounded(entry.application_identity.into_bytes())?,
    );
    response.outputs.insert(
        "PROGRAM.CONTENT".into(),
        bounded(entry.program_artifact.as_str().as_bytes().to_vec())?,
    );
    response.outputs.insert(
        "APPLICATION.VERSION".into(),
        bounded(
            format!(
                "{}.{}.{}",
                entry.major_version, entry.minor_version, entry.micro_version
            )
            .into_bytes(),
        )?,
    );
    Ok(response)
}

fn payload(request: &CicsRequest) -> Result<BoundedPayload, HostProblem> {
    if request.arguments.contains_key("CHANNEL") {
        return BoundedPayload::new(
            "mainframe-env.cics.channel@1",
            channel_argument(request)?.into_bytes(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted);
    }
    bounded(argument_bytes(request, "COMMAREA").unwrap_or_default())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "APPLICATION",
        "CHANNEL",
        "COMMAREA",
        "LENGTH",
        "MAJORVERSION",
        "MINORVERSION",
        "OPERATION",
        "OPTION.EXACTMATCH",
        "OPTION.MINIMUM",
        "OPTION.NOHANDLE",
        "PLATFORM",
        "RESP",
        "RESP2",
    ];
    if !request.arguments.contains_key("APPLICATION")
        || !request.arguments.contains_key("OPERATION")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("COMMAREA") && request.arguments.contains_key("CHANNEL")
        || request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || name.starts_with("OPTION.")
                    && (value.schema() != "mainframe-env.cics.option@1"
                        || !value.bytes().is_empty())
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn select_entry<'a>(
    entries: impl Iterator<Item = &'a CicsApplicationEntryDefinition>,
    major: Option<u32>,
    minor: Option<u32>,
    minimum: bool,
) -> Option<&'a CicsApplicationEntryDefinition> {
    entries
        .filter(|entry| match (major, minor) {
            (Some(major), Some(minor)) if minimum => {
                entry.major_version == major && entry.minor_version >= minor
            }
            (Some(major), Some(minor)) => {
                entry.major_version == major && entry.minor_version == minor
            }
            (None, None) => true,
            _ => false,
        })
        .max_by_key(|entry| {
            (
                entry.major_version,
                entry.minor_version,
                entry.micro_version,
            )
        })
}

fn current_platform(run: &Run) -> Option<String> {
    let value = run.invocation.bindings.get("cics.platform")?;
    (value.schema() == "mainframe-env.cics.platform@1")
        .then(|| std::str::from_utf8(value.bytes()).ok())
        .flatten()
        .and_then(|value| normalize_application_name(value).ok())
}

fn application_argument(request: &CicsRequest, name: &str) -> Result<String, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if !matches!(
        value.schema(),
        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    normalize_application_name(
        std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?,
    )
}

fn channel_argument(request: &CicsRequest) -> Result<String, HostProblem> {
    let raw = request
        .arguments
        .get("CHANNEL")
        .ok_or(HostProblem::Malformed)?;
    if !matches!(
        raw.schema(),
        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    let value = std::str::from_utf8(raw.bytes())
        .map_err(|_| condition("CHANNELERR", 122, 1))?
        .trim_end()
        .to_string();
    if !matches!(value.len(), 1..=16)
        || value.chars().any(char::is_whitespace)
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric()
                && !matches!(
                    character,
                    '$' | '@'
                        | '#'
                        | '/'
                        | '%'
                        | '&'
                        | '?'
                        | '!'
                        | ':'
                        | '|'
                        | '"'
                        | '='
                        | '¬'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                        | '.'
                        | '-'
                        | '_'
                )
        })
    {
        Err(condition("CHANNELERR", 122, 1))
    } else {
        Ok(value)
    }
}

fn optional_version(request: &CicsRequest, name: &str) -> Result<Option<u32>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            decimal_usize(value)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or(HostProblem::Malformed)
        })
        .transpose()
}
