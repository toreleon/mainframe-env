//! 3270 ISSUE controls over the existing durable terminal and BMS authority.

use super::*;
use mainframe_env_host_api::AccessIntent;

pub(in crate::service::handlers::terminal_control) fn eraseaup(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_eraseaup(request)?;
    super::super::validate_purge_message_context(run)?;
    if let Some(response) = receipt_for_request(service, run, request)? {
        return Ok(response);
    }
    // The source permits immediate return without WAIT. That asynchronous
    // form needs a retained pending terminal-control operation; fail closed
    // until it can join the existing terminal IO completion boundary.
    if !request.arguments.contains_key("OPTION.WAIT") {
        return Err(HostProblem::Unsupported);
    }
    let (current, map) = {
        let state = service.lock()?;
        let current = state
            .sessions
            .get(&run.session)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let map = current
            .mapset
            .as_ref()
            .zip(current.map.as_ref())
            .and_then(|(set, name)| state.maps.get(&(set.clone(), name.clone())))
            .cloned();
        (current, map)
    };
    if !current.connected {
        return Err(condition("NOTALLOC", 61));
    }
    let Some(map) = map else {
        return Err(HostProblem::Unsupported);
    };
    if current.field_values.is_empty() || map.fields.is_empty() {
        return Err(HostProblem::Unsupported);
    }
    service.authorize(
        run,
        "FACILITY",
        "CICS.TERMINAL.ERASEAUP",
        AccessIntent::Update,
    )?;
    let mut state = read_state(service, &run.session)?;
    let mut next = current.clone();
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let first_unprotected = map
        .fields
        .iter()
        .filter(|field| {
            !current
                .field_protection
                .get(&field.name)
                .copied()
                .unwrap_or(field.protected)
        })
        .map(|field| super::super::super::terminal_field_address(&current, &map, field))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .min();
    let mut erased = false;
    for (name, value) in &mut next.field_values {
        let Some(definition) = map
            .fields
            .iter()
            .find(|field| field.name.as_str() == name.as_str())
        else {
            return Err(HostProblem::Unsupported);
        };
        if !next
            .field_protection
            .get(name)
            .copied()
            .unwrap_or(definition.protected)
        {
            value.fill(0);
            next.field_modified.insert(name.clone(), false);
            erased = true;
        }
    }
    if !erased {
        return Err(HostProblem::Unsupported);
    }
    let mut image = Vec::new();
    for (name, value) in &next.field_values {
        field(&mut image, name.as_bytes())?;
        field(&mut image, value)?;
    }
    if image.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    next.screen = image;
    state.cursor = first_unprotected.ok_or(HostProblem::Unsupported)?;
    state.keyboard_unlocked = true;
    state.last_page = next.screen.clone();
    let receipt = base_receipt(run, request)?;
    commit(
        service,
        run,
        request,
        Some(&current),
        Some(&next),
        &state,
        &receipt,
    )
}

fn validate_eraseaup(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::IssueEraseAup
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.WAIT" | "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}
