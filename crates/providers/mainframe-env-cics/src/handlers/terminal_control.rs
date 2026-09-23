use super::super::{
    BmsMapDefinition, CicsService, Run, Session, argument_bytes, argument_optional, argument_text,
    bounded, decimal_payload, decode_map_payload, encode_symbolic_map_output, field,
    normalize_bms_input, symbolic_map_modified, symbolic_map_protection, symbolic_map_values,
};
use super::bms_map::map_fits_terminal;
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use std::collections::BTreeMap;

mod bms;
mod partition_set;
pub use bms::CicsBmsControlSnapshot;
pub(in crate::service) use bms::release_task as release_bms_message_for_task;
pub(in crate::service) use partition_set::release_task as release_partition_set_for_task;
pub use partition_set::{CicsPartitionDefinition, CicsPartitionSetDefinition};

#[derive(Clone, Debug, Default)]
pub(in crate::service) struct TerminalInput {
    pub(in crate::service) payload: Option<Vec<u8>>,
    pub(in crate::service) message_length: u32,
    pub(in crate::service) terminal_id: Option<String>,
}

struct DataOnlyAttributes {
    protection: BTreeMap<String, bool>,
    modified: BTreeMap<String, bool>,
}

impl TerminalInput {
    pub(in crate::service) fn identified(terminal_id: String) -> Self {
        Self {
            terminal_id: Some(terminal_id),
            ..Self::default()
        }
    }

    pub(in crate::service) fn replace(&mut self, payload: Vec<u8>) -> Result<(), HostProblem> {
        self.message_length = u32::try_from(payload.len())
            .ok()
            .filter(|length| *length <= 32_767)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.payload = Some(payload);
        Ok(())
    }
}

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
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::SendPartnset => partition_set::invoke(service, run, request),
        CicsOperation::ReceivePartn => partition_set::invoke_receive(service, run, request),
        CicsOperation::SendControl => bms::invoke_control(service, run, request),
        CicsOperation::SendPage => bms::invoke_page(service, run, request),
        CicsOperation::SendMap | CicsOperation::SendText => send(service, run, request),
        CicsOperation::ReceiveMap => receive(service, run, request),
        CicsOperation::PurgeMessage => bms::purge(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
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
    validate_send_request(request)?;
    let map_names = (request.operation == CicsOperation::SendMap)
        .then(|| map_names(request))
        .transpose()?;
    let mut payload = argument_bytes(request, "FROM")
        .or_else(|| argument_bytes(request, "DATA"))
        .unwrap_or_default();
    if let Some(length) = argument_optional(request, "LENGTH") {
        let length = length
            .trim()
            .parse::<i64>()
            .map_err(|_| HostProblem::Malformed)?;
        let length = usize::try_from(length).map_err(|_| length_problem(request.operation))?;
        if length > payload.len() {
            return Err(length_problem(request.operation));
        }
        payload.truncate(length);
    }
    let mut state = service.lock()?;
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let mut field_protection = None;
    let mut field_modified = None;
    let mut field_values = None;
    if request.operation == CicsOperation::SendMap {
        let (mapset, map) = map_names.as_ref().expect("SEND MAP names");
        let definition = state
            .maps
            .get(&(mapset.clone(), map.clone()))
            .ok_or(HostProblem::NotFound)?;
        if request.arguments.contains_key("OPTION.DATAONLY") {
            let attributes = data_only_attributes(definition, &payload, &current, mapset, map)?;
            field_protection = Some(attributes.protection);
            field_modified = Some(attributes.modified);
            field_values = Some(symbolic_map_values(definition, &payload)?);
            payload = encode_symbolic_map_output(definition, &payload)?;
        } else if payload.is_empty() {
            field_protection = Some(symbolic_map_protection(definition, &payload));
            field_modified = Some(symbolic_map_modified(definition, &payload));
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
            field_protection = Some(symbolic_map_protection(definition, &payload));
            field_modified = Some(symbolic_map_modified(definition, &payload));
            field_values = Some(symbolic_map_values(definition, &payload)?);
            payload = encode_symbolic_map_output(definition, &payload)?;
        } else if definition.fields.iter().any(|field| field.secret) {
            return Err(HostProblem::Unsupported);
        } else {
            field_protection = Some(symbolic_map_protection(definition, &payload));
            field_modified = Some(symbolic_map_modified(definition, &payload));
            field_values = Some(decode_map_payload(&payload, service.limits)?);
        }
    }
    if payload.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
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
    partition_set::persist_send(service, run, &current, &next)?;
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

fn data_only_attributes(
    map: &BmsMapDefinition,
    symbolic: &[u8],
    current: &Session,
    mapset: &str,
    map_name: &str,
) -> Result<DataOnlyAttributes, HostProblem> {
    let same_map =
        current.mapset.as_deref() == Some(mapset) && current.map.as_deref() == Some(map_name);
    let mut protection = BTreeMap::new();
    let mut modified = BTreeMap::new();
    for field in &map.fields {
        let offset = usize::try_from(field.attribute_offset.ok_or(HostProblem::Unsupported)?)
            .map_err(|_| HostProblem::ResourceExhausted)?;
        let attribute = symbolic
            .get(offset)
            .copied()
            .ok_or(HostProblem::Malformed)?;
        let name = field.name.to_ascii_uppercase();
        let (is_protected, is_modified) = match attribute {
            0 => (
                same_map
                    && current
                        .field_protection
                        .get(&name)
                        .copied()
                        .unwrap_or(false),
                same_map && current.field_modified.get(&name).copied().unwrap_or(false),
            ),
            0xc0 | 0xc1 | 0xc8 | 0xcc => (false, attribute & 0x01 != 0),
            0xf0 | 0xf1 | 0xf8 => (true, attribute & 0x01 != 0),
            _ => return Err(HostProblem::Malformed),
        };
        protection.insert(name.clone(), is_protected);
        modified.insert(name, is_modified);
    }
    Ok(DataOnlyAttributes {
        protection,
        modified,
    })
}

fn length_problem(operation: CicsOperation) -> HostProblem {
    if operation == CicsOperation::SendText {
        HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        }
    } else {
        HostProblem::Malformed
    }
}

fn validate_send_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const SEND_MAP_ALLOWED: &[&str] = &[
        "FROM",
        "LENGTH",
        "MAP",
        "MAPSET",
        "OPTION.CURSOR",
        "OPTION.DATAONLY",
        "OPTION.ERASE",
        "OPTION.FREEKB",
        "OPTION.MAPONLY",
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
    ];
    const SEND_TEXT_ALLOWED: &[&str] = &[
        "FROM",
        "LENGTH",
        "OPTION.ERASE",
        "OPTION.FREEKB",
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
    ];
    let allowed = match request.operation {
        CicsOperation::SendMap => SEND_MAP_ALLOWED,
        CicsOperation::SendText => SEND_TEXT_ALLOWED,
        _ => return Err(HostProblem::Malformed),
    };
    let required = match request.operation {
        CicsOperation::SendMap => "MAP",
        CicsOperation::SendText => "FROM",
        _ => return Err(HostProblem::Malformed),
    };
    if request.mutation.is_none()
        || !request.arguments.contains_key(required)
        || request.operation == CicsOperation::SendMap
            && request.arguments.contains_key("LENGTH")
            && !request.arguments.contains_key("FROM")
        || request.operation == CicsOperation::SendMap
            && request.arguments.contains_key("OPTION.MAPONLY")
            && (request.arguments.contains_key("FROM") || request.arguments.contains_key("LENGTH"))
        || request.operation == CicsOperation::SendMap
            && request.arguments.contains_key("OPTION.DATAONLY")
            && !request.arguments.contains_key("FROM")
        || request.arguments.contains_key("OPTION.DATAONLY")
            && request.arguments.contains_key("OPTION.MAPONLY")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || match name.as_str() {
                    "LENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                    "OPTION.CURSOR" | "OPTION.DATAONLY" | "OPTION.ERASE" | "OPTION.FREEKB"
                    | "OPTION.MAPONLY" | "OPTION.NOHANDLE" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                    "FROM" | "MAP" | "MAPSET" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.argument@1"
                            | "mainframe-env.cics.literal@1"
                            | "mainframe-env.cics.storage-value@1"
                    ),
                    _ => true,
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn receive(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_receive_request(request)?;
    let supplied_input = receive_from(request)?;
    let mut state = service.lock()?;
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    partition_set::require_intervening_send(service, run)?;
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
    let (disposition, target, payload, fields, next) = if let Some(input) = supplied_input {
        let fields = decode_map_payload(&input, service.limits)?;
        (CicsDisposition::Complete, None, input, fields, None)
    } else {
        let mut next = current.clone();
        next.version += 1;
        let (disposition, target, payload, fields) = if let Some(input) = next.input.payload.take()
        {
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
        (disposition, target, payload, fields, Some(next))
    };
    if let Some(next) = next {
        service.persist_session(&run.session, &next, Some(current.version))?;
        state.sessions.insert(run.session.clone(), next);
    }
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

fn validate_receive_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "FROM",
        "INTO",
        "LENGTH",
        "MAP",
        "MAPSET",
        "OPTION.NOHANDLE",
        "OPTION.TERMINAL",
        "RESP",
        "RESP2",
    ];
    if request.arguments.contains_key("LENGTH") && !request.arguments.contains_key("FROM")
        || request.arguments.contains_key("OPTION.TERMINAL")
            && request.arguments.contains_key("FROM")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "LENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                    "OPTION.NOHANDLE" | "OPTION.TERMINAL" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "INTO" | "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                    "FROM" | "MAP" | "MAPSET" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.argument@1"
                            | "mainframe-env.cics.literal@1"
                            | "mainframe-env.cics.storage-value@1"
                    ),
                    _ => true,
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn receive_from(request: &CicsRequest) -> Result<Option<Vec<u8>>, HostProblem> {
    let Some(mut input) = argument_bytes(request, "FROM") else {
        return Ok(None);
    };
    if let Some(length) = argument_optional(request, "LENGTH") {
        let length = length
            .trim()
            .parse::<i64>()
            .ok()
            .and_then(|length| usize::try_from(length).ok())
            .filter(|length| *length <= input.len())
            .ok_or(HostProblem::Malformed)?;
        input.truncate(length);
    }
    Ok(Some(input))
}

fn map_names(request: &CicsRequest) -> Result<(String, String), HostProblem> {
    let map = argument_text(request, "MAP")?
        .trim_end()
        .to_ascii_uppercase();
    let mapset = argument_optional(request, "MAPSET")
        .unwrap_or_else(|| map.clone())
        .trim_end()
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
