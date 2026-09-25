use super::selector::{decimal, option, source_data};
use super::*;
use state::{CicsOutboardKind, CicsOutboardRecord};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let owner = run.invocation.run_unit_id.as_str().to_owned();
    let mut task = state::read_task(service, &owner)?;
    let selected = selector::select(service, request, &task)?;
    let intent = if request.operation == CicsOperation::IssueNote {
        AccessIntent::Read
    } else {
        AccessIntent::Update
    };
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.OUTBOARD.{}", selected.name),
        intent,
    )?;
    let mut data = selected.data;
    let mut receipt = receipt(run, request)?;
    match request.operation {
        CicsOperation::IssueAdd => add(service, request, &selected.definition, &mut data)?,
        CicsOperation::IssueErase => erase(request, &selected.definition, &mut data)?,
        CicsOperation::IssueReplace => replace(service, request, &selected.definition, &mut data)?,
        CicsOperation::IssueNote => {
            if selected.definition.kind != CicsOutboardKind::Relative {
                return Err(condition("FUNCERR", 48, 0));
            }
            let next = next_number(&data)?;
            output(
                &mut receipt,
                "RIDFLD",
                "mainframe-env.cics.payload@1",
                next.to_be_bytes().to_vec(),
            );
        }
        CicsOperation::IssueSend => send(service, request, &selected.definition, &mut data)?,
        _ => return Err(HostProblem::InfrastructureFailure),
    }
    task.selected = Some(selected.name.clone());
    if matches!(
        request.operation,
        CicsOperation::IssueAdd
            | CicsOperation::IssueErase
            | CicsOperation::IssueReplace
            | CicsOperation::IssueSend
    ) {
        task.pending_destination = option(request, "NOWAIT").then(|| selected.name.clone());
        data.closed = false;
    }
    let changed = request.operation != CicsOperation::IssueNote;
    commit(
        service,
        run,
        request,
        changed.then_some((&selected.name, &selected.definition, &data)),
        &task,
        &receipt,
    )
}

fn add(
    service: &CicsService,
    request: &CicsRequest,
    definition: &CicsOutboardDestinationDefinition,
    data: &mut DataState,
) -> Result<(), HostProblem> {
    let bytes = source_data(request, service.limits.max_queue_bytes)?;
    match definition.kind {
        CicsOutboardKind::Sequential => {
            if bytes.len() != usize::from(definition.record_length)
                || request.arguments.contains_key("NUMREC")
                || request.arguments.contains_key("RIDFLD")
                || option(request, "RRN")
            {
                return Err(HostProblem::Malformed);
            }
            let number = next_number(data)?;
            push(data, definition, bytes, number, service)?;
        }
        CicsOutboardKind::Keyed => {
            if request.arguments.contains_key("NUMREC") || option(request, "RRN") {
                return Err(HostProblem::Malformed);
            }
            if bytes.len() != usize::from(definition.record_length) {
                return Err(HostProblem::Malformed);
            }
            let keys = keys(definition, &bytes)?;
            if let Some(rid) = request.arguments.get("RIDFLD")
                && rid.bytes() != keys[0]
            {
                return Err(condition("FUNCERR", 48, 0));
            }
            if data
                .records
                .iter()
                .any(|record| record.keys.iter().zip(keys.iter()).any(|(a, b)| a == b))
            {
                return Err(condition("FUNCERR", 48, 0));
            }
            let number = next_number(data)?;
            push(data, definition, bytes, number, service)?;
        }
        CicsOutboardKind::Relative => {
            if !option(request, "RRN") {
                return Err(HostProblem::Malformed);
            }
            let start = rrn(request)?;
            let count = decimal(request, "NUMREC")?.unwrap_or(1);
            if count == 0
                || count > service.limits.max_queue_records
                || bytes.len() != count.saturating_mul(usize::from(definition.record_length))
            {
                return Err(HostProblem::Malformed);
            }
            for index in 0..count {
                let number = start
                    .checked_add(u32::try_from(index).map_err(|_| HostProblem::ResourceExhausted)?)
                    .ok_or(HostProblem::ResourceExhausted)?;
                if data.records.iter().any(|record| record.number == number) {
                    return Err(condition("FUNCERR", 48, 0));
                }
            }
            for (index, chunk) in bytes
                .chunks(usize::from(definition.record_length))
                .enumerate()
            {
                push(
                    data,
                    definition,
                    chunk.to_vec(),
                    start + index as u32,
                    service,
                )?;
            }
            data.records.sort_by_key(|record| record.number);
        }
        CicsOutboardKind::Medium => return Err(HostProblem::Malformed),
    }
    Ok(())
}

fn replace(
    service: &CicsService,
    request: &CicsRequest,
    definition: &CicsOutboardDestinationDefinition,
    data: &mut DataState,
) -> Result<(), HostProblem> {
    let bytes = source_data(request, service.limits.max_queue_bytes)?;
    match definition.kind {
        CicsOutboardKind::Relative => {
            if !option(request, "RRN") {
                return Err(HostProblem::Malformed);
            }
            let start = rrn(request)?;
            let count = decimal(request, "NUMREC")?.unwrap_or(1);
            if count == 0
                || bytes.len() != count.saturating_mul(usize::from(definition.record_length))
            {
                return Err(HostProblem::Malformed);
            }
            for (index, chunk) in bytes
                .chunks(usize::from(definition.record_length))
                .enumerate()
            {
                let number = start
                    .checked_add(index as u32)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let record = data
                    .records
                    .iter_mut()
                    .find(|record| record.number == number)
                    .ok_or_else(|| condition("FUNCERR", 48, 0))?;
                record.data = chunk.to_vec();
            }
        }
        CicsOutboardKind::Keyed => {
            if request.arguments.contains_key("NUMREC")
                || option(request, "RRN")
                || bytes.len() != usize::from(definition.record_length)
            {
                return Err(HostProblem::Malformed);
            }
            let index = key_index(request, definition)?;
            let rid = keyed_rid(request, definition, index)?;
            let record = data
                .records
                .iter_mut()
                .find(|record| record.keys[index] == rid)
                .ok_or_else(|| condition("FUNCERR", 48, 0))?;
            record.data = bytes;
            record.keys = keys(definition, &record.data)?;
        }
        _ => return Err(condition("FUNCERR", 48, 0)),
    }
    state::validate_data_for_mutation(data, definition, service)?;
    Ok(())
}

fn erase(
    request: &CicsRequest,
    definition: &CicsOutboardDestinationDefinition,
    data: &mut DataState,
) -> Result<(), HostProblem> {
    match definition.kind {
        CicsOutboardKind::Relative => {
            if !option(request, "RRN") {
                return Err(HostProblem::Malformed);
            }
            let start = rrn(request)?;
            let count = decimal(request, "NUMREC")?.unwrap_or(1);
            if count == 0 {
                return Err(HostProblem::Malformed);
            }
            for index in 0..count {
                let number = start
                    .checked_add(index as u32)
                    .ok_or(HostProblem::ResourceExhausted)?;
                if !data.records.iter().any(|record| record.number == number) {
                    return Err(condition("FUNCERR", 48, 0));
                }
            }
            data.records.retain(|record| {
                record.number < start || u64::from(record.number) >= u64::from(start) + count as u64
            });
        }
        CicsOutboardKind::Keyed => {
            if request.arguments.contains_key("NUMREC") || option(request, "RRN") {
                return Err(HostProblem::Malformed);
            }
            let index = key_index(request, definition)?;
            let rid = keyed_rid(request, definition, index)?;
            let position = data
                .records
                .iter()
                .position(|record| record.keys[index] == rid)
                .ok_or_else(|| condition("FUNCERR", 48, 0))?;
            data.records.remove(position);
        }
        _ => return Err(condition("FUNCERR", 48, 0)),
    }
    Ok(())
}

fn send(
    service: &CicsService,
    request: &CicsRequest,
    definition: &CicsOutboardDestinationDefinition,
    data: &mut DataState,
) -> Result<(), HostProblem> {
    if !matches!(
        definition.kind,
        CicsOutboardKind::Sequential | CicsOutboardKind::Medium
    ) {
        return Err(condition("FUNCERR", 48, 0));
    }
    let bytes = source_data(request, service.limits.max_queue_bytes)?;
    if (definition.kind == CicsOutboardKind::Sequential
        && bytes.len() != usize::from(definition.record_length))
        || bytes.len() > usize::from(definition.record_length)
    {
        return Err(HostProblem::Malformed);
    }
    let number = next_number(data)?;
    push(data, definition, bytes, number, service)
}

fn push(
    data: &mut DataState,
    definition: &CicsOutboardDestinationDefinition,
    bytes: Vec<u8>,
    number: u32,
    service: &CicsService,
) -> Result<(), HostProblem> {
    if data.records.len() >= service.limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    data.records.push(CicsOutboardRecord {
        number,
        keys: keys(definition, &bytes)?,
        data: bytes,
    });
    Ok(())
}

fn next_number(data: &DataState) -> Result<u32, HostProblem> {
    data.records.last().map_or(Ok(0), |record| {
        record
            .number
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)
    })
}

fn keys(
    definition: &CicsOutboardDestinationDefinition,
    data: &[u8],
) -> Result<Vec<Vec<u8>>, HostProblem> {
    definition
        .indexes
        .iter()
        .map(|(offset, length)| {
            data.get(usize::from(*offset)..usize::from(*offset + *length))
                .map(|bytes| bytes.to_vec())
                .ok_or(HostProblem::Malformed)
        })
        .collect()
}

fn rrn(request: &CicsRequest) -> Result<u32, HostProblem> {
    let bytes = request
        .arguments
        .get("RIDFLD")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    Ok(u32::from_be_bytes(
        bytes.try_into().map_err(|_| HostProblem::Malformed)?,
    ))
}

fn key_index(
    request: &CicsRequest,
    definition: &CicsOutboardDestinationDefinition,
) -> Result<usize, HostProblem> {
    let number = decimal(request, "KEYNUMBER")?.unwrap_or(1);
    if number == 0 || number > definition.indexes.len() {
        return Err(HostProblem::Malformed);
    }
    Ok(number - 1)
}

fn keyed_rid(
    request: &CicsRequest,
    definition: &CicsOutboardDestinationDefinition,
    index: usize,
) -> Result<Vec<u8>, HostProblem> {
    let raw = request
        .arguments
        .get("RIDFLD")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let length = decimal(request, "KEYLENGTH")?.ok_or(HostProblem::Malformed)?;
    if length == 0 || length != usize::from(definition.indexes[index].1) || raw.len() < length {
        return Err(HostProblem::Malformed);
    }
    Ok(raw[..length].to_vec())
}
