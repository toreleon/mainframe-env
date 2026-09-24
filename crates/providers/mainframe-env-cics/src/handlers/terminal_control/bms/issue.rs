//! 3270 ISSUE controls over the existing durable terminal and BMS authority.

use super::super::super::issue_device::{IssueDeviceKind, IssueDeviceRecord};
use super::*;
use mainframe_env_host_api::AccessIntent;
use std::sync::atomic::Ordering;

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
    check_request_live(service, run)?;
    // The terminal executor completes this local buffer operation at its
    // start boundary. Without WAIT, that is one permitted completion order.
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
    check_terminal_owner(run, &current.run_unit, &current.principal)?;
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
    let response = commit(
        service,
        run,
        request,
        Some(&current),
        Some(&next),
        &state,
        &receipt,
    )?;
    after_commit(service, run, response)
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

pub(in crate::service::handlers::terminal_control) fn copy(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_copy(request)?;
    if let Some(response) = receipt_for_request(service, run, request)? {
        return Ok(response);
    }
    check_request_live(service, run)?;
    if request.arguments.contains_key("CTLCHAR") {
        return Err(HostProblem::Unsupported);
    }
    let source_bytes = request.arguments["TERMID"].bytes();
    if !(1..=4).contains(&source_bytes.len()) {
        return Err(condition("LENGERR", 22));
    }
    let source_id = std::str::from_utf8(source_bytes)
        .map_err(|_| HostProblem::Malformed)?
        .trim_end()
        .to_ascii_uppercase();
    if source_id.is_empty() {
        return Err(condition("LENGERR", 22));
    }
    if !source_id
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte))
    {
        return Err(HostProblem::Malformed);
    }
    let (current, source_key, source) = {
        let state = service.lock()?;
        let current = state
            .sessions
            .get(&run.session)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let (source_key, source) = state
            .sessions
            .iter()
            .find(|(_, session)| {
                session.input.terminal_id.as_deref() == Some(source_id.as_str())
                    && session.connected
            })
            .map(|(key, session)| (key.clone(), session.clone()))
            .ok_or_else(|| condition("NOTALLOC", 61))?;
        (current, source_key, source)
    };
    check_terminal_owner(run, &current.run_unit, &current.principal)?;
    if !current.connected {
        return Err(condition("NOTALLOC", 61));
    }
    let target_id = current
        .input
        .terminal_id
        .as_deref()
        .ok_or_else(|| condition("NOTALLOC", 61))?;
    let target_definition = IssueDeviceRecord::load(service.store.as_ref(), target_id)
        .map_err(store_error)?
        .ok_or_else(|| condition("NOTALLOC", 61))?;
    let source_definition = IssueDeviceRecord::load(service.store.as_ref(), &source_id)
        .map_err(store_error)?
        .ok_or_else(|| condition("NOTALLOC", 61))?;
    if target_definition.definition.kind != IssueDeviceKind::Display3270
        || source_definition.definition.kind != IssueDeviceKind::Display3270
        || target_definition.definition.control_unit != source_definition.definition.control_unit
    {
        return Err(condition("TERMERR", 81));
    }
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.ISSUE.DEVICE.{source_id}"),
        AccessIntent::Read,
    )?;
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.ISSUE.DEVICE.{target_id}"),
        AccessIntent::Update,
    )?;
    if source.screen.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let source_state = read_state(service, &source_key)?;
    let mut target_state = read_state(service, &run.session)?;
    let mut next = current.clone();
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    next.screen = source.screen;
    next.mapset = source.mapset;
    next.map = source.map;
    next.field_values = source.field_values;
    next.field_protection = source.field_protection;
    next.field_modified = source.field_modified;
    target_state.cursor = source_state.cursor;
    target_state.keyboard_unlocked = source_state.keyboard_unlocked;
    target_state.alternate_screen = source_state.alternate_screen;
    target_state.last_page = next.screen.clone();
    let receipt = base_receipt(run, request)?;
    let response = commit(
        service,
        run,
        request,
        Some(&current),
        Some(&next),
        &target_state,
        &receipt,
    )?;
    after_commit(service, run, response)
}

fn check_request_live(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if request_expired(service, run)? {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

fn check_terminal_owner(run: &Run, run_unit: &str, principal: &str) -> Result<(), HostProblem> {
    if !run_unit.is_empty()
        && (run_unit != run.invocation.run_unit_id.as_str()
            || principal != run.invocation.principal.id().as_str())
    {
        Err(condition("NOTALLOC", 61))
    } else {
        Ok(())
    }
}

fn request_expired(service: &CicsService, run: &Run) -> Result<bool, HostProblem> {
    service
        .replay_clock
        .as_ref()
        .map(|clock| {
            clock
                .now_tick()
                .map(|tick| tick >= run.invocation.deadline_tick)
        })
        .transpose()
        .map(|expired| expired.unwrap_or(false))
}

fn after_commit(
    service: &CicsService,
    run: &Run,
    response: CicsResponse,
) -> Result<CicsResponse, HostProblem> {
    if run.invocation.cancellation_requested()
        || request_expired(service, run)?
        || service
            .replay_unknown_after_persist
            .swap(false, Ordering::SeqCst)
    {
        Err(HostProblem::UnknownOutcome)
    } else {
        Ok(response)
    }
}

fn validate_copy(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::IssueCopy
        || !request.arguments.contains_key("TERMID")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "TERMID" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "CTLCHAR" => value.schema() != "mainframe-env.cics.storage-value@1",
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.WAIT" | "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        Err(HostProblem::Malformed)
    } else {
        if request
            .arguments
            .get("CTLCHAR")
            .is_some_and(|value| value.bytes().len() != 1)
        {
            Err(condition("LENGERR", 22))
        } else {
            Ok(())
        }
    }
}

fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}
