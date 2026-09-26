//! Public task-channel command profile over the owner-scoped port.

use super::{channel::ChannelPort, scope::OwnerIdentity, state::ContainerDatatype};
use crate::service::handlers::bts_lifecycle::BtsLifecycleStore;
use crate::service::{CicsService, Run};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

pub(super) fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

pub(super) fn name(request: &CicsRequest, key: &str) -> Result<Option<String>, HostProblem> {
    let Some(value) = request.arguments.get(key) else {
        return Ok(None);
    };
    if !matches!(
        value.schema(),
        "mainframe-env.cics.argument@1"
            | "mainframe-env.cics.literal@1"
            | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
    let text = text.trim_end_matches(' ');
    if !super::state::valid_name(text, 16) {
        return Err(condition(
            if matches!(key, "CHANNEL" | "TOCHANNEL") {
                "CHANNELERR"
            } else {
                "CONTAINERERR"
            },
            if matches!(key, "CHANNEL" | "TOCHANNEL") {
                122
            } else {
                110
            },
            1,
        ));
    }
    Ok(Some(text.to_owned()))
}

pub(super) fn number(request: &CicsRequest, key: &str) -> Result<Option<i64>, HostProblem> {
    let Some(value) = request.arguments.get(key) else {
        return Ok(None);
    };
    if key == "FLENGTH"
        && request.operation == CicsOperation::GetContainer
        && value.schema() == "mainframe-env.cics.argument@1"
    {
        return Ok(None);
    }
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    let parsed = std::str::from_utf8(value.bytes())
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| condition("LENGERR", 22, 1))?;
    Ok(Some(parsed))
}

pub(super) fn output(
    response: &mut CicsResponse,
    key: &str,
    schema: &'static str,
    bytes: Vec<u8>,
) -> Result<(), HostProblem> {
    response.outputs.insert(
        key.into(),
        BoundedPayload::new(schema, bytes, InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(())
}

fn channel_error(problem: HostProblem) -> HostProblem {
    match problem {
        HostProblem::NotFound => condition("CHANNELERR", 122, 2),
        HostProblem::Unauthorized => condition("CHANNELERR", 122, 6),
        HostProblem::Malformed => condition("INVREQ", 16, 1),
        other => other,
    }
}

pub(super) fn container_error(problem: HostProblem) -> HostProblem {
    match problem {
        HostProblem::NotFound => condition("CONTAINERERR", 110, 1),
        HostProblem::Malformed => condition("INVREQ", 16, 1),
        other => other,
    }
}

pub(in crate::service::handlers) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let operation = request.operation;
    if matches!(
        operation,
        CicsOperation::GetContainer64 | CicsOperation::PutContainer64
    ) {
        let allowed: &[&str] = if operation == CicsOperation::GetContainer64 {
            &[
                "CONTAINER",
                "CHANNEL",
                "ABI64",
                "INTO",
                "INTO.MAXLENGTH",
                "FLENGTH",
                "BYTEOFFSET",
                "CCSID",
                "INTOCCSID",
                "INTOCODEPAGE",
                "CONVERTST",
                "OPTION.NODATA",
                "RESP",
                "RESP2",
                "OPTION.NOHANDLE",
            ]
        } else {
            &[
                "CONTAINER",
                "CHANNEL",
                "ABI64",
                "FROM",
                "FLENGTH",
                "DATATYPE",
                "FROMCCSID",
                "RESP",
                "RESP2",
                "OPTION.APPEND",
                "OPTION.NOHANDLE",
            ]
        };
        if request
            .arguments
            .keys()
            .any(|key| !allowed.contains(&key.as_str()))
        {
            return Err(HostProblem::Unsupported);
        }
        if request.arguments.get("ABI64").is_none_or(|value| {
            value.schema() != "mainframe-env.cics.literal@1"
                || value.bytes() != b"mainframe-env.cics-amode64-nonle@1"
        }) {
            return Err(HostProblem::Unsupported);
        }
        if operation == CicsOperation::GetContainer64 {
            let nodata = request.arguments.contains_key("OPTION.NODATA");
            let convertst = request.arguments.get("CONVERTST");
            if request.arguments.contains_key("INTO") == nodata
                || nodata && !request.arguments.contains_key("FLENGTH")
                || nodata && request.arguments.contains_key("BYTEOFFSET")
                || nodata && request.arguments.contains_key("INTO.MAXLENGTH")
                || nodata
                    && request
                        .arguments
                        .get("FLENGTH")
                        .is_some_and(|value| value.schema() != "mainframe-env.cics.argument@1")
                || request.arguments.contains_key("CCSID") && convertst.is_none()
                || convertst.is_some_and(|value| {
                    value.schema() != "mainframe-env.cics.literal@1"
                        || value.bytes() != b"NOCONVERT"
                })
                || request.arguments.contains_key("INTOCCSID")
                    && request.arguments.contains_key("INTOCODEPAGE")
                || request.arguments.get("INTOCODEPAGE").is_some_and(|value| {
                    value.schema() != "mainframe-env.cics.literal@1" || value.bytes() != b"37"
                })
            {
                return Err(HostProblem::Unsupported);
            }
        } else if request.arguments.get("DATATYPE").is_some_and(|value| {
            value.schema() != "mainframe-env.cics.literal@1"
                || !matches!(value.bytes(), b"BIT" | b"CHAR")
        }) || request
            .arguments
            .get("DATATYPE")
            .is_some_and(|value| value.bytes() == b"BIT")
            && request.arguments.contains_key("FROMCCSID")
        {
            return Err(HostProblem::Unsupported);
        }
        let key = if operation == CicsOperation::GetContainer64 {
            "INTO"
        } else {
            "FROM"
        };
        let data = if operation == CicsOperation::GetContainer64
            && request.arguments.contains_key("OPTION.NODATA")
        {
            None
        } else {
            Some(request.arguments.get(key).ok_or(HostProblem::Malformed)?)
        };
        if let Some(data) = data {
            if data.schema() == "mainframe-env.cics.invalid-pointer64@1" {
                return Err(condition("INVREQ", 16, 1));
            }
            if data.schema() == "mainframe-env.cics.length-error64@1" {
                return Err(condition("LENGERR", 22, 1));
            }
            let expected = if operation == CicsOperation::GetContainer64 {
                "mainframe-env.cics.pointer64@1"
            } else {
                "mainframe-env.cics.storage64-value@1"
            };
            if data.schema() != expected {
                return Err(HostProblem::Unsupported);
            }
            if operation == CicsOperation::GetContainer64 && data.bytes().len() != 8 {
                return Err(condition("INVREQ", 16, 1));
            }
        }
        let mut routed = request.clone();
        routed.arguments.remove("ABI64");
        routed.arguments.remove("CONVERTST");
        if routed.arguments.remove("INTOCODEPAGE").is_some() {
            routed.arguments.insert(
                "INTOCCSID".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.decimal@1",
                    b"37".to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
        routed.operation = if operation == CicsOperation::GetContainer64 {
            CicsOperation::GetContainer
        } else {
            CicsOperation::PutContainer
        };
        for key in ["FLENGTH", "BYTEOFFSET"] {
            if let Some(value) = if operation == CicsOperation::GetContainer64
                && key == "FLENGTH"
                && request
                    .arguments
                    .get(key)
                    .is_some_and(|value| value.schema() == "mainframe-env.cics.argument@1")
            {
                None
            } else {
                number(request, key)?
            } {
                if value > i64::from(i32::MAX) || value < i64::from(i32::MIN) {
                    return Err(condition(
                        if key == "FLENGTH" {
                            "LENGERR"
                        } else {
                            "INVREQ"
                        },
                        if key == "FLENGTH" { 22 } else { 16 },
                        1,
                    ));
                }
                if operation == CicsOperation::GetContainer64 && value < 0 {
                    routed.arguments.insert(
                        key.into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.decimal@1",
                            b"0".to_vec(),
                            InvocationLimits::default(),
                        )
                        .map_err(|_| HostProblem::ResourceExhausted)?,
                    );
                }
            }
        }
        let value = if operation == CicsOperation::GetContainer64 {
            BoundedPayload::new(
                "mainframe-env.cics.argument@1",
                b"INTO64".to_vec(),
                InvocationLimits::default(),
            )
        } else {
            BoundedPayload::new(
                "mainframe-env.cics.storage-value@1",
                data.ok_or(HostProblem::Malformed)?.bytes().to_vec(),
                InvocationLimits::default(),
            )
        }
        .map_err(|_| HostProblem::ResourceExhausted)?;
        if !request.arguments.contains_key("OPTION.NODATA") {
            routed.arguments.insert(key.into(), value);
        }
        return invoke(service, run, &routed);
    }
    if matches!(
        operation,
        CicsOperation::DeleteContainer
            | CicsOperation::GetContainer
            | CicsOperation::MoveContainer
            | CicsOperation::PutContainer
    ) && (request.arguments.keys().any(|key| {
        matches!(
            key.as_str(),
            "ACTIVITY"
                | "FROMACTIVITY"
                | "TOACTIVITY"
                | "OPTION.PROCESS"
                | "OPTION.ACQPROCESS"
                | "OPTION.ACQACTIVITY"
                | "OPTION.FROMPROCESS"
                | "OPTION.TOPROCESS"
        )
    }) || !request.arguments.contains_key("CHANNEL")
        && !request.arguments.contains_key("TOCHANNEL")
        && BtsLifecycleStore::new(service.store.as_ref())
            .active_context(
                run.invocation.run_unit_id.as_str(),
                run.invocation.execution_id.as_str(),
                run.invocation.principal.id().as_str(),
            )?
            .is_some())
    {
        return super::bts::invoke(service, run, request);
    }
    if request.arguments.keys().any(|key| {
        matches!(
            key.as_str(),
            "ACTIVITY"
                | "FROMACTIVITY"
                | "TOACTIVITY"
                | "OPTION.PROCESS"
                | "OPTION.ACQPROCESS"
                | "OPTION.ACQACTIVITY"
                | "OPTION.FROMPROCESS"
                | "OPTION.TOPROCESS"
        )
    }) {
        return Err(HostProblem::Unsupported);
    }
    let allowed: &[&str] = match operation {
        CicsOperation::DeleteChannel => &["CHANNEL", "RESP", "RESP2", "OPTION.NOHANDLE"],
        CicsOperation::DeleteContainer => {
            &["CONTAINER", "CHANNEL", "RESP", "RESP2", "OPTION.NOHANDLE"]
        }
        CicsOperation::GetContainer => &[
            "CONTAINER",
            "CHANNEL",
            "INTO",
            "INTO.MAXLENGTH",
            "SET",
            "SET.MAXLENGTH",
            "FLENGTH",
            "BYTEOFFSET",
            "CCSID",
            "INTOCCSID",
            "INTOCODEPAGE",
            "CONVERTST",
            "OPTION.NODATA",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::MoveContainer => &[
            "CONTAINER",
            "AS",
            "CHANNEL",
            "TOCHANNEL",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::PutContainer => &[
            "CONTAINER",
            "CHANNEL",
            "FROM",
            "FLENGTH",
            "DATATYPE",
            "FROMCCSID",
            "FROMCODEPAGE",
            "OPTION.APPEND",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        CicsOperation::QueryChannel => &[
            "CHANNEL",
            "CONTAINERCNT",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        _ => return Err(HostProblem::Unsupported),
    };
    if request
        .arguments
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
        || request.arguments.iter().any(|(key, value)| {
            key.starts_with("OPTION.")
                && (value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty())
        })
    {
        return Err(HostProblem::Unsupported);
    }
    if operation == CicsOperation::GetContainer
        && (request.arguments.contains_key("INTOCODEPAGE")
            || request.arguments.contains_key("CONVERTST"))
        || operation == CicsOperation::PutContainer
            && request.arguments.contains_key("FROMCODEPAGE")
    {
        return Err(HostProblem::Unsupported);
    }
    let explicit_channel = name(request, "CHANNEL")?;
    let channel = explicit_channel
        .or_else(|| run.current_program.channel.clone())
        .ok_or_else(|| condition("INVREQ", 16, 1))?;
    let container = name(request, "CONTAINER")?;
    let to_channel = name(request, "TOCHANNEL")?;
    let as_name = name(request, "AS")?;
    if operation == CicsOperation::DeleteChannel {
        if channel == "DFHTRANSACTION" {
            return Err(condition("CHANNELERR", 122, 5));
        }
        if run.current_program.channel.as_deref() == Some(&channel) {
            return Err(condition("CHANNELERR", 122, 4));
        }
    }
    let run_unit = run.invocation.run_unit_id.as_str().to_owned();
    let execution = run.invocation.execution_id.as_str().to_owned();
    let principal = run.invocation.principal.id().as_str().to_owned();
    let program = run
        .current_program
        .current
        .clone()
        .unwrap_or_else(|| "TASK".into());
    let identity = OwnerIdentity {
        run_unit: &run_unit,
        execution: &execution,
        principal: &principal,
    };
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
    let mut authorize = |class: &str, resource: &str, intent: AccessIntent| {
        service.authorize(run, class, resource, intent)
    };
    let mut port =
        ChannelPort::new_with_program(service.store.as_ref(), identity, &program, &mut authorize);
    let replay = request
        .mutation
        .as_ref()
        .map(|mutation| mutation.idempotency_key.as_str());
    match operation {
        CicsOperation::DeleteChannel => {
            port.delete_channel(&channel, replay.ok_or(HostProblem::MissingIdempotency)?)
                .map_err(channel_error)?;
        }
        CicsOperation::DeleteContainer => {
            port.count(&channel).map_err(channel_error)?;
            port.delete_container(
                &channel,
                container.as_deref().ok_or(HostProblem::Malformed)?,
                replay.ok_or(HostProblem::MissingIdempotency)?,
            )
            .map_err(container_error)?;
        }
        CicsOperation::GetContainer => {
            port.count(&channel).map_err(channel_error)?;
            let value = port
                .get_value(
                    &channel,
                    container.as_deref().ok_or(HostProblem::Malformed)?,
                )
                .map_err(container_error)?;
            if let Some(ccsid) = number(request, "INTOCCSID")?
                && value.ccsid != u16::try_from(ccsid).ok()
            {
                return Err(HostProblem::Unsupported);
            }
            let offset = usize::try_from(number(request, "BYTEOFFSET")?.unwrap_or(0))
                .map_err(|_| condition("INVREQ", 16, 1))?;
            let data = value.bytes.get(offset..).unwrap_or_default();
            let actual = data.len();
            let nodata = request.arguments.contains_key("OPTION.NODATA");
            let into = request.arguments.contains_key("INTO");
            let set = request.arguments.contains_key("SET");
            if usize::from(nodata) + usize::from(into) + usize::from(set) != 1 {
                return Err(condition("INVREQ", 16, 1));
            }
            let maximum = number(request, "FLENGTH")?
                .map(|length| usize::try_from(length).map_err(|_| condition("LENGERR", 22, 1)))
                .transpose()?;
            if into {
                let capacity = number(request, "INTO.MAXLENGTH")?
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(HostProblem::Malformed)?;
                let copied = data.len().min(capacity).min(maximum.unwrap_or(capacity));
                if copied < data.len() {
                    response.condition = "LENGERR".into();
                    response.response = 22;
                    response.response2 = 1;
                }
                output(
                    &mut response,
                    "INTO",
                    "mainframe-env.cics.payload@1",
                    data[..copied].to_vec(),
                )?;
            }
            if set {
                let capacity = number(request, "SET.MAXLENGTH")?
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(HostProblem::Malformed)?;
                if data.len() > capacity {
                    return Err(HostProblem::ResourceExhausted);
                }
                output(
                    &mut response,
                    "SET",
                    "mainframe-env.cics.payload@1",
                    data.to_vec(),
                )?;
            }
            if request.arguments.contains_key("FLENGTH") {
                output(
                    &mut response,
                    "FLENGTH",
                    "mainframe-env.cics.decimal@1",
                    actual.to_string().into_bytes(),
                )?;
            }
            if request.arguments.contains_key("CCSID") {
                output(
                    &mut response,
                    "CCSID",
                    "mainframe-env.cics.decimal@1",
                    value.ccsid.unwrap_or(0).to_string().into_bytes(),
                )?;
            }
        }
        CicsOperation::MoveContainer => {
            port.count(&channel).map_err(channel_error)?;
            let to_channel = to_channel.unwrap_or_else(|| channel.clone());
            port.move_to(
                &channel,
                container.as_deref().ok_or(HostProblem::Malformed)?,
                &to_channel,
                as_name.as_deref().ok_or(HostProblem::Malformed)?,
                replay.ok_or(HostProblem::MissingIdempotency)?,
            )
            .map_err(container_error)?;
        }
        CicsOperation::PutContainer => {
            let from = request
                .arguments
                .get("FROM")
                .ok_or(HostProblem::Malformed)?;
            if !matches!(
                from.schema(),
                "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
            ) {
                return Err(HostProblem::Malformed);
            }
            let length = number(request, "FLENGTH")?.unwrap_or(from.bytes().len() as i64);
            let length = usize::try_from(length).map_err(|_| condition("LENGERR", 22, 1))?;
            if length > from.bytes().len() {
                return Err(condition("LENGERR", 22, 1));
            }
            let datatype = request
                .arguments
                .get("DATATYPE")
                .map(|value| match value.bytes() {
                    b"BIT" => Ok(ContainerDatatype::Bit),
                    b"CHAR" => Ok(ContainerDatatype::Character),
                    _ => Err(condition("INVREQ", 16, 1)),
                })
                .transpose()?;
            let ccsid = number(request, "FROMCCSID")?;
            let ccsid = match ccsid {
                Some(value) => {
                    Some(u16::try_from(value).map_err(|_| condition("CCSIDERR", 123, 1))?)
                }
                None => None,
            };
            if datatype == Some(ContainerDatatype::Bit) && ccsid.is_some() {
                return Err(condition("INVREQ", 16, 1));
            }
            if ccsid.is_some_and(|codepage| codepage != 37) {
                return Err(HostProblem::Unsupported);
            }
            port.put_value(
                &channel,
                container.as_deref().ok_or(HostProblem::Malformed)?,
                &from.bytes()[..length],
                datatype,
                ccsid,
                request.arguments.contains_key("OPTION.APPEND"),
                replay.ok_or(HostProblem::MissingIdempotency)?,
            )
            .map_err(container_error)?;
        }
        CicsOperation::QueryChannel => {
            let count = match port.count(&channel) {
                Ok(count) => count,
                Err(HostProblem::NotFound) if channel == "DFHTRANSACTION" => 0,
                Err(problem) => return Err(channel_error(problem)),
            };
            output(
                &mut response,
                "CONTAINERCNT",
                "mainframe-env.cics.decimal@1",
                count.to_string().into_bytes(),
            )?;
        }
        _ => unreachable!(),
    }
    Ok(response)
}
