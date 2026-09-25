use super::*;
use crate::service::handlers::transform_control;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let handler = run.invocation.bindings.get("cics.soap.handler");
    if handler.is_none_or(|value| {
        value.schema() != "mainframe-env.cics.soap-handler@1" || value.bytes() != b"supplied"
    }) {
        return Err(condition("INVREQ", 16, 3));
    }
    let channel = channel(service, run, request).map_err(|_| condition("INVREQ", 16, 3))?;
    let level = soap_level(service, &channel)?;
    if run
        .invocation
        .bindings
        .get("cics.channel.readonly")
        .is_some_and(|value| {
            value.schema() == "mainframe-env.cics.channel-readonly@1" && value.bytes() == b"true"
        })
    {
        return Err(condition("CHANNELERR", 122, 3));
    }
    let mut state = state::load(service, run, &channel)?;
    match request.operation {
        CicsOperation::SoapFaultCreate => create(service, request, level, &mut state)?,
        CicsOperation::SoapFaultAdd => add(service, request, level, &mut state)?,
        CicsOperation::SoapFaultDelete => {
            if !state.fields.contains_key("SOAP.CODE") {
                return Err(condition("NOTFND", 13, 2));
            }
            state.fields.retain(|name, _| !name.starts_with("SOAP."));
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    }
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.SOAP.CHANNEL.{channel}"),
        AccessIntent::Update,
    )?;
    let response = normal(service, run)?;
    state::persist(
        service,
        run,
        request,
        retention_tick,
        &channel,
        &state,
        &response,
    )?;
    Ok(response)
}

fn soap_level(service: &CicsService, channel: &str) -> Result<u8, HostProblem> {
    let state = service.lock()?;
    let container = state
        .transform_containers
        .get(&(channel.into(), "DFHWS-SOAPLEVEL".into()))
        .ok_or_else(|| condition("INVREQ", 16, 3))?;
    if container.mode != super::super::transform_control::CicsTransformContainerMode::Bit
        || container.bytes.len() != 4
    {
        return Err(condition("INVREQ", 16, 3));
    }
    match container.bytes.as_slice() {
        [0, 0, 0, 1] => Ok(1),
        [0, 0, 0, 2] => Ok(2),
        _ => Err(condition("INVREQ", 16, 3)),
    }
}

fn create(
    service: &CicsService,
    request: &CicsRequest,
    level: u8,
    state: &mut WebState,
) -> Result<(), HostProblem> {
    if value(request, "FAULTCODE").is_some() == value(request, "FAULTCODESTR").is_some() {
        return Err(HostProblem::Malformed);
    }
    let code = if let Some(value) = text(request, "FAULTCODE", 8)? {
        match (level, value.as_str()) {
            (1, "CLIENT" | "SENDER") => "Client".to_string(),
            (1, "SERVER" | "RECEIVER") => "Server".to_string(),
            (2, "CLIENT" | "SENDER") => "Sender".to_string(),
            (2, "SERVER" | "RECEIVER") => "Receiver".to_string(),
            _ => return Err(condition("INVREQ", 16, 11)),
        }
    } else {
        if level != 1 {
            return Err(condition("INVREQ", 16, 11));
        }
        let name = utf8_data(request, "FAULTCODESTR", 64, Some("FAULTCODELEN"), 5)?
            .ok_or(HostProblem::Malformed)?;
        if !valid_qname(&name) {
            return Err(condition("INVREQ", 16, 11));
        }
        name
    };
    let fault_string = utf8_data(request, "FAULTSTRING", 2056, Some("FAULTSTRLEN"), 6)?
        .ok_or(HostProblem::Malformed)?;
    let language = language(request)?;
    let role = utf8_data(request, "ROLE", 2056, Some("ROLELENGTH"), 7)?;
    let actor = utf8_data(request, "FAULTACTOR", 2056, Some("FAULTACTLEN"), 8)?;
    let detail = utf8_data(
        request,
        "DETAIL",
        service.limits.max_screen_bytes,
        Some("DETAILLENGTH"),
        9,
    )?;
    if role.as_ref().is_some_and(|value| !valid_uri(value))
        || actor.as_ref().is_some_and(|value| !valid_uri(value))
    {
        return Err(condition("INVREQ", 16, 11));
    }
    if let Some(detail) = detail.as_ref() {
        let wrapped = format!("<detail>{detail}</detail>");
        if !transform_control::valid_web_xml(&wrapped, service.limits) {
            return Err(condition("INVREQ", 16, 13));
        }
    }
    from_ccsid(request)?;
    state.fields.retain(|name, _| !name.starts_with("SOAP."));
    state.fields.insert("SOAP.LEVEL".into(), vec![level]);
    state.fields.insert("SOAP.CODE".into(), code.into_bytes());
    state
        .fields
        .insert(format!("SOAP.STRING.{language}"), fault_string.into_bytes());
    if let Some(role) = role.filter(|_| level == 2) {
        state.fields.insert("SOAP.ROLE".into(), role.into_bytes());
    }
    if let Some(actor) = actor {
        state.fields.insert("SOAP.ACTOR".into(), actor.into_bytes());
    }
    if let Some(detail) = detail {
        state
            .fields
            .insert("SOAP.DETAIL".into(), detail.into_bytes());
    }
    Ok(())
}

fn add(
    service: &CicsService,
    request: &CicsRequest,
    level: u8,
    state: &mut WebState,
) -> Result<(), HostProblem> {
    if !state.fields.contains_key("SOAP.CODE")
        || state.fields.get("SOAP.LEVEL").map(Vec::as_slice) != Some(&[level][..])
    {
        return Err(condition("INVREQ", 16, 7));
    }
    let language = language(request)?;
    let fault_string = utf8_data(request, "FAULTSTRING", 2056, Some("FAULTSTRLEN"), 6)?;
    let subcode = utf8_data(request, "SUBCODESTR", 64, Some("SUBCODELEN"), 10)?;
    if fault_string.is_none() && subcode.is_none() {
        return Err(HostProblem::Malformed);
    }
    if let Some(subcode) = subcode {
        if level == 2 {
            if !valid_qname(&subcode) {
                return Err(condition("INVREQ", 16, 11));
            }
            state
                .fields
                .insert("SOAP.SUBCODE".into(), subcode.into_bytes());
        }
    }
    if let Some(fault_string) = fault_string {
        if level == 2
            || !state
                .fields
                .keys()
                .any(|name| name.starts_with("SOAP.STRING."))
        {
            state
                .fields
                .insert(format!("SOAP.STRING.{language}"), fault_string.into_bytes());
        } else if level == 1 {
            let key = state
                .fields
                .keys()
                .find(|name| name.starts_with("SOAP.STRING."))
                .cloned()
                .ok_or(HostProblem::InfrastructureFailure)?;
            state.fields.insert(key, fault_string.into_bytes());
        }
    }
    from_ccsid(request)?;
    if state.fields.len() > 64
        || state.fields.values().map(Vec::len).sum::<usize>() > service.limits.max_transform_bytes
    {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}

fn language(request: &CicsRequest) -> Result<String, HostProblem> {
    let raw = text(request, "NATLANG", 8)?.unwrap_or_else(|| "en".into());
    if raw.len() > 8
        || !raw
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b' ')
    {
        return Err(condition("INVREQ", 16, 11));
    }
    let language = raw.trim_end_matches(' ').to_ascii_uppercase();
    if language.is_empty() {
        return Err(condition("INVREQ", 16, 11));
    }
    Ok(language)
}

fn valid_qname(value: &str) -> bool {
    let mut parts = value.split(':');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    };
    valid(first) && second.is_none_or(valid)
}

pub(super) fn valid_uri(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        && value.contains(':')
}
