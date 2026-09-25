use super::super::partition_set::normalized_name;
use super::control::{apply_frame, condition, decimal, has, page_pointer};
use super::*;
use mainframe_env_host_api::{AccessIntent, CicsOperation};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    super::super::validate_purge_message_context(run)?;
    if let Some(response) = receipt_for_request(service, run, request)? {
        return Ok(response);
    }
    let current = service
        .lock()?
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let mut state = read_state(service, &run.session)?;
    let Some(message) = state.message.take() else {
        return Err(condition("INVREQ", 16));
    };
    if request.arguments.contains_key("FMHPARM") {
        // The local terminal is a 3270/8775, not a 3650 outboard formatter.
        return Err(condition("INVREQ", 16));
    }
    if request.arguments.contains_key("SET") != (message.mode == 2) {
        return Err(condition("INVREQ", 16));
    }
    let trailer = request
        .arguments
        .get("TRAILER")
        .map(|value| trailer(value.bytes()))
        .transpose()?;
    service.authorize(run, "FACILITY", "CICS.TERMINAL.PAGE", AccessIntent::Update)?;
    let mut next = current.clone();
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    for frame in &message.frames {
        apply_frame(&mut state, &mut next, frame)?;
    }
    if !message.payload.is_empty() {
        if message.payload.len() > service.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        next.screen = message.payload;
    }
    if let Some(trailer) = trailer {
        if next
            .screen
            .len()
            .checked_add(trailer.len())
            .is_none_or(|size| size > service.limits.max_screen_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        next.screen.extend_from_slice(&trailer);
    }
    let page = next.screen.clone();
    let mut receipt = base_receipt(run, request)?;
    if message.mode == 1 {
        let queued_bytes = state
            .queued_pages
            .iter()
            .try_fold(page.len(), |size, item| {
                size.checked_add(item.len())
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
        if state.queued_pages.len() >= service.limits.max_queue_records
            || queued_bytes > service.limits.max_queue_bytes
        {
            return Err(HostProblem::ResourceExhausted);
        }
        state.queued_pages.push(page.clone());
        if !has(request, "AUTOPAGE") {
            next.screen = current.screen.clone();
        }
    } else if message.mode == 2 {
        let capacity = decimal(request, "SET.MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
        receipt.set = Some(page_pointer(&page, capacity)?);
        receipt.condition = "RETPAGE".into();
        receipt.response = 32;
        next.screen = current.screen.clone();
    }
    state.last_page = page;
    if has(request, "RELEASE") {
        receipt.disposition = CicsDisposition::Returned;
        receipt.next_transaction = request
            .arguments
            .get("TRANSID")
            .map(|value| normalized_name(value.bytes(), 4))
            .transpose()?;
    }
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

pub(super) fn purge(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    super::super::validate_purge_message_request(request)?;
    super::super::validate_purge_message_context(run)?;
    if let Some(response) = receipt_for_request(service, run, request)? {
        return Ok(response);
    }
    service.authorize(run, "FACILITY", "CICS.TERMINAL.PAGE", AccessIntent::Update)?;
    let mut state = read_state(service, &run.session)?;
    state.message = None;
    let receipt = base_receipt(run, request)?;
    commit(service, run, request, None, None, &state, &receipt)
}

fn trailer(bytes: &[u8]) -> Result<Vec<u8>, HostProblem> {
    if bytes.len() < 4 {
        return Err(condition("INVREQ", 16));
    }
    let length = i16::from_be_bytes([bytes[0], bytes[1]]);
    if length < 0
        || bytes[2..4] != [0, 0]
        || usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_add(4))
            != Some(bytes.len())
    {
        return Err(condition("INVREQ", 16));
    }
    Ok(bytes[4..].to_vec())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const OPTIONS: &[&str] = &[
        "RELEASE",
        "RETAIN",
        "AUTOPAGE",
        "CURRENT",
        "ALL",
        "NOAUTOPAGE",
        "OPERPURGE",
        "LAST",
        "NOHANDLE",
    ];
    let arguments = &request.arguments;
    if request.operation != CicsOperation::SendPage
        || request.mutation.is_none()
        || arguments.contains_key("RESP2") && !arguments.contains_key("RESP")
        || has(request, "RELEASE") && has(request, "RETAIN")
        || has(request, "AUTOPAGE") && has(request, "NOAUTOPAGE")
        || arguments.contains_key("TRANSID") && !has(request, "RELEASE")
        || [has(request, "CURRENT"), has(request, "ALL")]
            .into_iter()
            .filter(|value| *value)
            .count()
            > 1
        || arguments.iter().any(|(name, value)| {
            if let Some(option) = name.strip_prefix("OPTION.") {
                return !OPTIONS.contains(&option)
                    || value.schema() != "mainframe-env.cics.option@1"
                    || !value.bytes().is_empty();
            }
            if matches!(name.as_str(), "TRANSID" | "FMHPARM") {
                return !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                );
            }
            if name == "TRAILER" {
                return value.schema() != "mainframe-env.cics.storage-value@1";
            }
            if name == "SET.MAXLENGTH" {
                return value.schema() != "mainframe-env.cics.decimal@1";
            }
            if matches!(name.as_str(), "SET" | "RESP" | "RESP2") {
                return value.schema() != "mainframe-env.cics.argument@1";
            }
            true
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
