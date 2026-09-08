use super::super::{
    CicsService, Run, argument_bytes, argument_text, bounded, decimal_payload, decode_map_payload,
    encode_symbolic_map_output, field, normalize_bms_input, symbolic_map_modified,
    symbolic_map_protection, symbolic_map_values,
};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use std::collections::BTreeMap;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::SendMap | CicsOperation::SendText => send(service, run, request),
        CicsOperation::ReceiveMap => receive(service, run),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn send(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let mut state = service.lock()?;
    let mut payload = argument_bytes(request, "FROM")
        .or_else(|| argument_bytes(request, "DATA"))
        .unwrap_or_default();
    let mut field_protection = None;
    let mut field_modified = None;
    let mut field_values = None;
    if request.operation == CicsOperation::SendMap {
        let mapset = argument_text(request, "MAPSET")?;
        let map = argument_text(request, "MAP")?;
        let definition = state
            .maps
            .get(&(mapset.to_ascii_uppercase(), map.to_ascii_uppercase()))
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
    next.version += 1;
    next.screen = payload.clone();
    if request.operation == CicsOperation::SendMap {
        next.mapset = Some(argument_text(request, "MAPSET")?.to_ascii_uppercase());
        next.map = Some(argument_text(request, "MAP")?.to_ascii_uppercase());
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

fn receive(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
    let mut state = service.lock()?;
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let mut next = current.clone();
    next.version += 1;
    let (disposition, payload, fields) = if let Some(input) = next.input.take() {
        let fields = decode_map_payload(&input, service.limits)?;
        (CicsDisposition::Complete, input, fields)
    } else {
        next.suspended = true;
        (CicsDisposition::Suspended, Vec::new(), BTreeMap::new())
    };
    service.persist_session(&run.session, &next, Some(current.version))?;
    state.sessions.insert(run.session.clone(), next);
    let mut response = service.response(run, disposition, "NORMAL", 0, 0, None, None, payload)?;
    response.aid = current.aid;
    for (name, value) in fields {
        let input_length = value.len();
        let value = current
            .mapset
            .as_ref()
            .zip(current.map.as_ref())
            .and_then(|(mapset, map)| state.maps.get(&(mapset.clone(), map.clone())))
            .and_then(|map| {
                map.fields
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
