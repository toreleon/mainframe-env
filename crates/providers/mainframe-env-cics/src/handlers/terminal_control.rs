use super::super::{
    CicsService, Run, argument_bytes, argument_optional, argument_text, bounded, decimal_payload,
    decode_map_payload, encode_symbolic_map_output, field, normalize_bms_input,
    symbolic_map_modified, symbolic_map_protection, symbolic_map_values,
};
use super::bms_map::map_fits_terminal;
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use std::collections::BTreeMap;

pub(in crate::service) const fn valid_aid(aid: u8) -> bool {
    matches!(
        aid,
        0x4a..=0x4c
            | 0x6a..=0x6e
            | 0x7a..=0x7f
            | 0xc1..=0xc9
            | 0xe6..=0xe7
            | 0xf1..=0xf9
    )
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::SendMap | CicsOperation::SendText => send(service, run, request),
        CicsOperation::ReceiveMap => receive(service, run, request),
        CicsOperation::PurgeMessage => purge_message(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn purge_message(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_purge_message_request(request)?;
    validate_purge_message_context(run)?;
    // The local runtime exposes no ACCUM/page-building route, so its reachable
    // logical-message state is empty. Purging that state is deliberately
    // idempotent and must not erase the already displayed terminal image.
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

fn validate_purge_message_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if request
        .arguments
        .iter()
        .any(|(name, value)| match name.as_str() {
            "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            }
            _ => true,
        })
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_purge_message_context(run: &Run) -> Result<(), HostProblem> {
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

fn send(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let map_names = (request.operation == CicsOperation::SendMap)
        .then(|| map_names(request))
        .transpose()?;
    let mut state = service.lock()?;
    let mut payload = argument_bytes(request, "FROM")
        .or_else(|| argument_bytes(request, "DATA"))
        .unwrap_or_default();
    let mut field_protection = None;
    let mut field_modified = None;
    let mut field_values = None;
    if request.operation == CicsOperation::SendMap {
        let (mapset, map) = map_names.as_ref().expect("SEND MAP names");
        let definition = state
            .maps
            .get(&(mapset.clone(), map.clone()))
            .ok_or(HostProblem::NotFound)?;
        field_protection = Some(symbolic_map_protection(definition, &payload));
        field_modified = Some(symbolic_map_modified(definition, &payload));
        if payload.is_empty() {
            let mut values = BTreeMap::new();
            for item in &definition.fields {
                field(&mut payload, item.name.as_bytes())?;
                field(&mut payload, &item.initial)?;
                values.insert(item.name.to_ascii_uppercase(), item.initial.clone());
            }
            field_values = Some(values);
        } else if definition
            .fields
            .iter()
            .all(|field| field.output_offset.is_some())
        {
            field_values = Some(symbolic_map_values(definition, &payload)?);
            payload = encode_symbolic_map_output(definition, &payload)?;
        } else if definition.fields.iter().any(|field| field.secret) {
            return Err(HostProblem::Unsupported);
        } else {
            field_values = Some(decode_map_payload(&payload, service.limits)?);
        }
    }
    if payload.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let mut next = current.clone();
    if request.operation == CicsOperation::SendMap {
        let (mapset, map) = map_names.as_ref().expect("SEND MAP names");
        let definition = state
            .maps
            .get(&(mapset.clone(), map.clone()))
            .ok_or(HostProblem::NotFound)?;
        if !map_fits_terminal(&current, definition) {
            return Err(HostProblem::Condition {
                name: "INVMPSZ".into(),
                response: 38,
                response2: 0,
            });
        }
    }
    next.version += 1;
    next.screen = payload.clone();
    if request.operation == CicsOperation::SendMap {
        let (mapset, map) = map_names.expect("SEND MAP names");
        next.mapset = Some(mapset);
        next.map = Some(map);
        next.field_protection = field_protection.unwrap_or_default();
        next.field_modified = field_modified.unwrap_or_default();
        next.field_values = field_values.unwrap_or_default();
    }
    service.persist_session(&run.session, &next, Some(current.version))?;
    state.sessions.insert(run.session.clone(), next);
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        payload,
    )
}

fn receive(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let mut state = service.lock()?;
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let requested_names = if request.arguments.contains_key("MAP") {
        Some(map_names(request)?)
    } else {
        current.mapset.clone().zip(current.map.clone())
    };
    let definition = requested_names
        .as_ref()
        .and_then(|names| state.maps.get(names))
        .cloned();
    if requested_names.is_some() && definition.is_none() {
        return Err(HostProblem::NotFound);
    }
    if definition
        .as_ref()
        .is_some_and(|definition| !map_fits_terminal(&current, definition))
    {
        return Err(HostProblem::Condition {
            name: "INVMPSZ".into(),
            response: 38,
            response2: 0,
        });
    }
    let mut next = current.clone();
    next.version += 1;
    let (disposition, target, payload, fields) = if let Some(input) = next.input.take() {
        let fields = decode_map_payload(&input, service.limits)?;
        let target = aid_handler_target(&run.aid_handlers, current.aid);
        (
            if target.is_some() {
                CicsDisposition::Handler
            } else {
                CicsDisposition::Complete
            },
            target,
            input,
            fields,
        )
    } else {
        next.suspended = true;
        (
            CicsDisposition::Suspended,
            None,
            Vec::new(),
            BTreeMap::new(),
        )
    };
    service.persist_session(&run.session, &next, Some(current.version))?;
    state.sessions.insert(run.session.clone(), next);
    let mut response = service.response(run, disposition, "NORMAL", 0, 0, target, None, payload)?;
    response.aid = current.aid;
    for (name, value) in fields {
        let input_length = value.len();
        let value = definition
            .as_ref()
            .and_then(|definition| {
                definition
                    .fields
                    .iter()
                    .find(|field| field.name.eq_ignore_ascii_case(&name))
            })
            .map_or(value.clone(), |field| normalize_bms_input(field, &value));
        response
            .outputs
            .insert(format!("BMS.{name}"), bounded(value.clone())?);
        response.outputs.insert(
            format!("BMS.{name}.LENGTH"),
            decimal_payload(
                i64::try_from(input_length).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
    }
    Ok(response)
}

fn map_names(request: &CicsRequest) -> Result<(String, String), HostProblem> {
    let map = argument_text(request, "MAP")?.trim().to_ascii_uppercase();
    let mapset = argument_optional(request, "MAPSET")
        .unwrap_or_else(|| map.clone())
        .trim()
        .to_ascii_uppercase();
    if [&map, &mapset].iter().any(|name| {
        name.is_empty() || name.len() > 7 || !name.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }) {
        return Err(HostProblem::Malformed);
    }
    Ok((mapset, map))
}

fn aid_handler_target(handlers: &BTreeMap<String, String>, aid: u8) -> Option<String> {
    let name = aid_name(aid)?;
    if let Some(label) = handlers.get(name) {
        return (!label.is_empty()).then(|| label.clone());
    }
    if matches!(name, "CLEAR" | "PA1" | "PA2" | "PA3") || name.starts_with("PF") {
        return handlers
            .get("ANYKEY")
            .filter(|label| !label.is_empty())
            .cloned();
    }
    None
}

fn aid_name(aid: u8) -> Option<&'static str> {
    Some(match aid {
        0x6d => "CLEAR",
        0x6a => "CLRPARTN",
        0x7d => "ENTER",
        0x7e => "LIGHTPEN",
        0xe6 | 0xe7 => "OPERID",
        0x6c => "PA1",
        0x6e => "PA2",
        0x6b => "PA3",
        0xf1..=0xf9 => return pf_name(aid - 0xf0),
        0x7a..=0x7c => return pf_name(aid - 0x70),
        0xc1..=0xc9 => return pf_name(aid - 0xb4),
        0x4a..=0x4c => return pf_name(aid - 0x34),
        0x7f => "TRIGGER",
        _ => return None,
    })
}

fn pf_name(number: u8) -> Option<&'static str> {
    const NAMES: [&str; 24] = [
        "PF1", "PF2", "PF3", "PF4", "PF5", "PF6", "PF7", "PF8", "PF9", "PF10", "PF11", "PF12",
        "PF13", "PF14", "PF15", "PF16", "PF17", "PF18", "PF19", "PF20", "PF21", "PF22", "PF23",
        "PF24",
    ];
    number
        .checked_sub(1)
        .and_then(|index| NAMES.get(usize::from(index)))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_aid_tombstones_precede_anykey_and_every_supported_byte_is_named() {
        let handlers = BTreeMap::from([
            ("ANYKEY".into(), "ANY-HANDLER".into()),
            ("ENTER".into(), "ENTER-HANDLER".into()),
            ("PF10".into(), String::new()),
        ]);
        assert_eq!(
            aid_handler_target(&handlers, 0xf1).as_deref(),
            Some("ANY-HANDLER")
        );
        assert_eq!(
            aid_handler_target(&handlers, 0x6d).as_deref(),
            Some("ANY-HANDLER")
        );
        assert_eq!(aid_handler_target(&handlers, 0x7a), None);
        assert_eq!(
            aid_handler_target(&handlers, 0x7d).as_deref(),
            Some("ENTER-HANDLER")
        );
        for aid in [
            0x4a, 0x4b, 0x4c, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f,
            0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xe6, 0xe7, 0xf1, 0xf2, 0xf3,
            0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9,
        ] {
            assert!(aid_name(aid).is_some(), "{aid:02x}");
        }
        assert!(aid_name(0x00).is_none());
    }
}
