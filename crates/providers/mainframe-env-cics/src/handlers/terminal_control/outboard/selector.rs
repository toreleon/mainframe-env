use super::*;
use state::{normalized_name, read_data, read_definition};

pub(super) struct Selected {
    pub name: String,
    pub definition: CicsOutboardDestinationDefinition,
    pub data: DataState,
}

pub(super) fn select(
    service: &CicsService,
    request: &CicsRequest,
    task: &TaskState,
) -> Result<Selected, HostProblem> {
    let media = media_name(request)?;
    let destid = request
        .arguments
        .get("DESTID")
        .map(|value| normalized_name(value.bytes(), 8))
        .transpose()?;
    if media.is_some() && destid.is_some() {
        return Err(HostProblem::Malformed);
    }
    let name = media
        .or(destid.clone())
        .or_else(|| task.selected.clone())
        .ok_or_else(|| condition("SELNERR", 47, 0))?;
    if destid.is_none() && task.selected.is_none() && !name.starts_with('M') {
        return Err(condition("SELNERR", 47, 0));
    }
    let definition = read_definition(service, &name)?.ok_or_else(|| condition("SELNERR", 47, 0))?;
    if let Some(volume) = request.arguments.get("VOLUME") {
        let volume = normalized_name(volume.bytes(), 6)?;
        if definition.volume.as_deref() != Some(volume.as_str()) {
            return Err(condition("SELNERR", 47, 0));
        }
    }
    if let Some(length) = decimal(request, "DESTIDLENG")?
        && length != name.len()
    {
        return Err(HostProblem::Malformed);
    }
    if let Some(length) = decimal(request, "VOLUMELENG")?
        && length != definition.volume.as_ref().map_or(0, String::len)
    {
        return Err(HostProblem::Malformed);
    }
    let data = read_data(service, &name, &definition)?;
    if data.closed
        && destid.is_none()
        && !matches!(
            request.operation,
            CicsOperation::IssueEnd | CicsOperation::IssueAbort
        )
    {
        return Err(condition("SELNERR", 47, 0));
    }
    Ok(Selected {
        name,
        definition,
        data,
    })
}

fn media_name(request: &CicsRequest) -> Result<Option<String>, HostProblem> {
    let flags = [
        ("CONSOLE", "MCON"),
        ("PRINT", "MPRT"),
        ("CARD", "MCRD"),
        ("WPMEDIA1", "MWP1"),
        ("WPMEDIA2", "MWP2"),
        ("WPMEDIA3", "MWP3"),
        ("WPMEDIA4", "MWP4"),
    ];
    let selected = flags
        .iter()
        .filter(|(name, _)| option(request, name))
        .collect::<Vec<_>>();
    if selected.len() > 1 {
        return Err(HostProblem::Malformed);
    }
    let Some((_, prefix)) = selected.first() else {
        return Ok(None);
    };
    let subaddress = decimal(request, "SUBADDR")?.unwrap_or(0);
    if subaddress > 15 {
        return Err(HostProblem::Malformed);
    }
    let subaddress = if subaddress == 15 { 0 } else { subaddress };
    Ok(Some(format!("{prefix}{subaddress:02}")))
}

pub(super) fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let operation = request.operation;
    if !matches!(
        operation,
        CicsOperation::IssueAbort
            | CicsOperation::IssueAdd
            | CicsOperation::IssueEnd
            | CicsOperation::IssueErase
            | CicsOperation::IssueNote
            | CicsOperation::IssueQuery
            | CicsOperation::IssueReceive
            | CicsOperation::IssueReplace
            | CicsOperation::IssueSend
            | CicsOperation::IssueWait
    ) || request.mutation.is_none()
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    let dataset = !matches!(operation, CicsOperation::IssueReceive);
    let writes = matches!(
        operation,
        CicsOperation::IssueAdd | CicsOperation::IssueReplace | CicsOperation::IssueSend
    );
    let indexed = matches!(
        operation,
        CicsOperation::IssueErase | CicsOperation::IssueReplace
    );
    let media = matches!(
        operation,
        CicsOperation::IssueAbort
            | CicsOperation::IssueEnd
            | CicsOperation::IssueSend
            | CicsOperation::IssueWait
    );
    let record = matches!(
        operation,
        CicsOperation::IssueAdd | CicsOperation::IssueErase | CicsOperation::IssueReplace
    );
    let output = operation == CicsOperation::IssueReceive;
    let mut media_count = 0usize;
    for (name, value) in &request.arguments {
        if let Some(option_name) = name.strip_prefix("OPTION.") {
            let permitted = match option_name {
                "NOHANDLE" => true,
                "DEFRESP" | "NOWAIT" => record || operation == CicsOperation::IssueSend,
                "RRN" => record || operation == CicsOperation::IssueNote,
                "CONSOLE" | "PRINT" | "CARD" | "WPMEDIA1" | "WPMEDIA2" | "WPMEDIA3"
                | "WPMEDIA4" => {
                    media_count += 1;
                    media
                }
                _ => false,
            };
            if !permitted
                || value.schema() != "mainframe-env.cics.option@1"
                || !value.bytes().is_empty()
            {
                return Err(HostProblem::Malformed);
            }
            continue;
        }
        let permitted = match name.as_str() {
            "DESTID" | "VOLUME" => {
                dataset
                    && matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    )
            }
            "DESTIDLENG" | "VOLUMELENG" => {
                dataset && value.schema() == "mainframe-env.cics.decimal@1"
            }
            "SUBADDR" => media && value.schema() == "mainframe-env.cics.decimal@1",
            "FROM" => writes && value.schema() == "mainframe-env.cics.storage-value@1",
            "LENGTH" => (writes || output) && value.schema() == "mainframe-env.cics.decimal@1",
            "NUMREC" => record && value.schema() == "mainframe-env.cics.decimal@1",
            "KEYLENGTH" | "KEYNUMBER" => {
                indexed && value.schema() == "mainframe-env.cics.decimal@1"
            }
            "RIDFLD" => {
                (record && value.schema() == "mainframe-env.cics.storage-value@1")
                    || operation == CicsOperation::IssueNote
                        && value.schema() == "mainframe-env.cics.argument@1"
            }
            "INTO" | "SET" => output && value.schema() == "mainframe-env.cics.argument@1",
            "INTO.MAXLENGTH" | "SET.MAXLENGTH" => {
                output && value.schema() == "mainframe-env.cics.decimal@1"
            }
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            _ => false,
        };
        if !permitted {
            return Err(HostProblem::Malformed);
        }
    }
    if media_count > 1
        || media_count > 0 && request.arguments.contains_key("DESTID")
        || request.arguments.contains_key("DESTIDLENG") && !request.arguments.contains_key("DESTID")
        || request.arguments.contains_key("VOLUMELENG") && !request.arguments.contains_key("VOLUME")
        || request.arguments.contains_key("VOLUME") && media_count > 0
        || request.arguments.contains_key("SUBADDR") && media_count == 0
        || option(request, "RRN")
            && (request.arguments.contains_key("KEYLENGTH")
                || request.arguments.contains_key("KEYNUMBER"))
        || writes
            && (!request.arguments.contains_key("FROM")
                || !request.arguments.contains_key("LENGTH"))
        || indexed && !request.arguments.contains_key("RIDFLD")
        || operation == CicsOperation::IssueNote
            && (!request.arguments.contains_key("RIDFLD") || !option(request, "RRN"))
        || output
            && (!request.arguments.contains_key("LENGTH")
                || request.arguments.contains_key("INTO") == request.arguments.contains_key("SET"))
    {
        return Err(HostProblem::Malformed);
    }
    for name in [
        "DESTIDLENG",
        "VOLUMELENG",
        "SUBADDR",
        "LENGTH",
        "NUMREC",
        "KEYLENGTH",
        "KEYNUMBER",
    ] {
        if name == "LENGTH" && operation == CicsOperation::IssueReceive {
            let bytes = request
                .arguments
                .get(name)
                .ok_or(HostProblem::Malformed)?
                .bytes();
            let value = std::str::from_utf8(bytes)
                .map_err(|_| HostProblem::Malformed)?
                .parse::<i64>()
                .map_err(|_| HostProblem::Malformed)?;
            if value < i16::MIN as i64 || value > i16::MAX as i64 {
                return Err(HostProblem::Malformed);
            }
        } else if let Some(value) = decimal(request, name)?
            && value > u16::MAX as usize
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

pub(super) fn option(request: &CicsRequest, name: &str) -> bool {
    request.arguments.contains_key(&format!("OPTION.{name}"))
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

pub(super) fn source_data(request: &CicsRequest, max: usize) -> Result<Vec<u8>, HostProblem> {
    let from = request
        .arguments
        .get("FROM")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let length = decimal(request, "LENGTH")?.ok_or(HostProblem::Malformed)?;
    if length == 0 || length > from.len() || length > max {
        return Err(HostProblem::Malformed);
    }
    Ok(from[..length].to_vec())
}
