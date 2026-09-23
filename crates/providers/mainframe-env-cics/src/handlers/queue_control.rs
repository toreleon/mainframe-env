use super::super::{
    CicsService, Reader, Run, TransientQueue, argument_bytes, argument_text, bounded,
    decimal_payload, decode_transient, mutation_problem, store_error,
};
use super::transient_data::{
    CicsTransientDataQueueDefinition, CicsTransientDataQueueKind, CicsTransientDataQueueOpen,
    TransientDataState, condition, persist_queue,
};
use mainframe_env_host_api::{
    AccessIntent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, CicsResponse,
    HostProblem,
};

mod temporary_storage;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::WriteTransientData => write(service, run, request),
        CicsOperation::ReadTransientData => read(service, run, request),
        CicsOperation::DeleteTransientData => delete(service, run, request),
        CicsOperation::DeleteTemporaryStorage => {
            temporary_storage::delete_temporary(service, run, request)
        }
        CicsOperation::ReadTemporaryStorage => {
            temporary_storage::read_temporary(service, run, request)
        }
        CicsOperation::WriteTemporaryStorage => {
            temporary_storage::write_temporary(service, run, request)
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn signed_decimal(request: &CicsRequest, name: &str) -> Result<Option<i64>, HostProblem> {
    let Some(value) = request.arguments.get(name) else {
        return Ok(None);
    };
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map(Some)
        .map_err(|_| HostProblem::Malformed)
}

fn read(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "QUEUE" | "SYSID" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "INTO" | "SET" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
                "INTO.MAXLENGTH" | "SET.MAXLENGTH" | "LENGTH" => {
                    value.schema() != "mainframe-env.cics.decimal@1"
                }
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
        || request.arguments.contains_key("INTO") == request.arguments.contains_key("SET")
        || request.arguments.contains_key("INTO")
            != request.arguments.contains_key("INTO.MAXLENGTH")
        || request.arguments.contains_key("SET") != request.arguments.contains_key("SET.MAXLENGTH")
    {
        return Err(HostProblem::Malformed);
    }
    let queue = queue_name(request)?;
    validate_local_system(run, request)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TD.{queue}"),
        AccessIntent::Read,
    )?;
    let set = request.arguments.contains_key("SET");
    let destination_maximum = signed_decimal(
        request,
        if set {
            "SET.MAXLENGTH"
        } else {
            "INTO.MAXLENGTH"
        },
    )?
    .and_then(|value| usize::try_from(value).ok())
    .filter(|value| *value != 0)
    .ok_or(HostProblem::Malformed)?;
    let requested = signed_decimal(request, "LENGTH")?;
    if requested.is_some_and(|value| value < 0) {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        });
    }
    let mut state = service.lock()?;
    let definition = state.transient.definition(&queue, service.limits)?;
    validate_read_definition(&definition)?;
    let current = state.transient.queues.get(&queue).cloned().ok_or_else(|| {
        if state.transient.definitions.contains_key(&queue) {
            condition("QZERO", 23)
        } else {
            condition("QIDERR", 44)
        }
    })?;
    let (_, original) = current
        .records
        .first()
        .cloned()
        .ok_or_else(|| HostProblem::Condition {
            name: "QZERO".into(),
            response: 23,
            response2: 0,
        })?;
    if take_io_failure(
        &mut state.transient,
        CicsOperation::ReadTransientData,
        &queue,
    ) {
        let mut next = current;
        next.records.remove(0);
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        persist_queue(
            service.store.as_ref(),
            queue.clone(),
            &next,
            Some(next.version - 1),
        )
        .map_err(mutation_problem)?;
        state.transient.bytes = state
            .transient
            .bytes
            .checked_sub(original.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        let empty = next.records.is_empty();
        state.transient.queues.insert(queue, next);
        return Err(if empty {
            condition("QZERO", 23)
        } else {
            condition("IOERR", 17)
        });
    }
    let maximum = if set {
        destination_maximum
    } else {
        requested
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(destination_maximum)
            .min(destination_maximum)
    };
    if set && original.len() > maximum {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        });
    }
    let truncated = !set && maximum < original.len();
    let zero_length = requested == Some(0);
    let mut value = original.clone();
    value.truncate(maximum);
    let mut next = current;
    next.records.remove(0);
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    persist_queue(
        service.store.as_ref(),
        queue.clone(),
        &next,
        Some(next.version - 1),
    )
    .map_err(mutation_problem)?;
    state.transient.bytes = state
        .transient
        .bytes
        .checked_sub(original.len())
        .ok_or(HostProblem::InfrastructureFailure)?;
    state.transient.queues.insert(queue, next);
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        if truncated || zero_length {
            "LENGERR"
        } else {
            "NORMAL"
        },
        if truncated || zero_length { 22 } else { 0 },
        0,
        None,
        None,
        value,
    )?;
    if request.arguments.contains_key("LENGTH") {
        response.outputs.insert(
            "LENGTH".into(),
            decimal_payload(
                i64::try_from(original.len()).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
    }
    if set {
        response.outputs.insert("SET".into(), bounded(original)?);
        response.payload = bounded(Vec::new())?;
    }
    Ok(response)
}

fn validate_write_definition(
    definition: &CicsTransientDataQueueDefinition,
    record_length: usize,
) -> Result<(), HostProblem> {
    validate_enabled(definition)?;
    match (definition.kind, definition.open) {
        (CicsTransientDataQueueKind::Intrapartition, None) => {
            if definition
                .record_size
                .is_some_and(|maximum| record_length > maximum)
            {
                return Err(condition("LENGERR", 22));
            }
        }
        (CicsTransientDataQueueKind::Extrapartition, Some(CicsTransientDataQueueOpen::Output)) => {
            if definition.record_size != Some(record_length) {
                return Err(condition("LENGERR", 22));
            }
        }
        (CicsTransientDataQueueKind::Extrapartition, Some(CicsTransientDataQueueOpen::Input)) => {
            return Err(condition("INVREQ", 16));
        }
        (CicsTransientDataQueueKind::Extrapartition, Some(CicsTransientDataQueueOpen::Closed)) => {
            return Err(condition("NOTOPEN", 19));
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    }
    Ok(())
}

fn validate_read_definition(
    definition: &CicsTransientDataQueueDefinition,
) -> Result<(), HostProblem> {
    validate_enabled(definition)?;
    match (definition.kind, definition.open) {
        (CicsTransientDataQueueKind::Intrapartition, None)
        | (CicsTransientDataQueueKind::Extrapartition, Some(CicsTransientDataQueueOpen::Input)) => {
            Ok(())
        }
        (CicsTransientDataQueueKind::Extrapartition, Some(CicsTransientDataQueueOpen::Output)) => {
            Err(condition("INVREQ", 16))
        }
        (CicsTransientDataQueueKind::Extrapartition, Some(CicsTransientDataQueueOpen::Closed)) => {
            Err(condition("NOTOPEN", 19))
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn validate_delete_definition(
    definition: &CicsTransientDataQueueDefinition,
) -> Result<(), HostProblem> {
    validate_enabled(definition)?;
    if definition.kind == CicsTransientDataQueueKind::Extrapartition {
        Err(condition("INVREQ", 16))
    } else {
        Ok(())
    }
}

fn validate_enabled(definition: &CicsTransientDataQueueDefinition) -> Result<(), HostProblem> {
    if definition.enabled {
        Ok(())
    } else {
        Err(condition("DISABLED", 84))
    }
}

#[cfg(test)]
fn take_io_failure(state: &mut TransientDataState, operation: CicsOperation, queue: &str) -> bool {
    if state
        .io_failure
        .as_ref()
        .is_some_and(|(failed_operation, failed_queue)| {
            *failed_operation == operation && failed_queue == queue
        })
    {
        state.io_failure.take();
        true
    } else {
        false
    }
}

#[cfg(not(test))]
fn take_io_failure(
    _state: &mut TransientDataState,
    _operation: CicsOperation,
    _queue: &str,
) -> bool {
    false
}

fn temporary_queue_name(request: &CicsRequest) -> Result<String, HostProblem> {
    if request.arguments.contains_key("QUEUE") == request.arguments.contains_key("QNAME") {
        return Err(HostProblem::Malformed);
    }
    let (name, maximum) = if request.arguments.contains_key("QNAME") {
        ("QNAME", 16)
    } else {
        ("QUEUE", 8)
    };
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if !matches!(
        value.schema(),
        "mainframe-env.cics.argument@1"
            | "mainframe-env.cics.literal@1"
            | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    if !value.bytes().is_empty() && value.bytes().iter().all(|byte| *byte == 0) {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 0,
        });
    }
    let reserved = value
        .bytes()
        .first()
        .is_some_and(|byte| matches!(byte, 0xfa..=0xff))
        || value.bytes().starts_with(b"**")
        || value.bytes().starts_with(b"$$")
        || value
            .bytes()
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"DF"));
    if reserved {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 0,
        });
    }
    let queue = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim()
        .to_ascii_uppercase();
    if queue.is_empty()
        || queue.len() > maximum
        || !queue
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(HostProblem::Malformed);
    }
    Ok(queue)
}

fn validate_local_system(run: &Run, request: &CicsRequest) -> Result<(), HostProblem> {
    validate_temporary_system(run, request, 0)
}

fn validate_temporary_system(
    run: &Run,
    request: &CicsRequest,
    response2: i32,
) -> Result<(), HostProblem> {
    let Some(value) = request.arguments.get("SYSID") else {
        return Ok(());
    };
    if !matches!(
        value.schema(),
        "mainframe-env.cics.argument@1"
            | "mainframe-env.cics.literal@1"
            | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    let system = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim()
        .to_ascii_uppercase();
    if system.is_empty()
        || system.len() > 4
        || !system.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(HostProblem::Malformed);
    }
    if !system.eq_ignore_ascii_case(&run.sysid) {
        return Err(HostProblem::Condition {
            name: "SYSIDERR".into(),
            response: 53,
            response2,
        });
    }
    Ok(())
}

fn queue_name(request: &CicsRequest) -> Result<String, HostProblem> {
    let queue = argument_text(request, "QUEUE")
        .or_else(|_| argument_text(request, "TDQUEUE"))?
        .trim()
        .to_ascii_uppercase();
    if queue.is_empty()
        || queue.len() > 16
        || !queue
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(HostProblem::Malformed);
    }
    Ok(queue)
}

fn write(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let queue = queue_name(request)?;
    validate_local_system(run, request)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TD.{queue}"),
        AccessIntent::Update,
    )?;
    let mut state = service.lock()?;
    let mut value = argument_bytes(request, "FROM").ok_or(HostProblem::Malformed)?;
    if let Some(length) = argument_bytes(request, "LENGTH") {
        let length = std::str::from_utf8(&length)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|length| *length <= value.len())
            .ok_or_else(|| HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 0,
            })?;
        value.truncate(length);
    }
    let definition = state.transient.definition(&queue, service.limits)?;
    validate_write_definition(&definition, value.len())?;
    if take_io_failure(
        &mut state.transient,
        CicsOperation::WriteTransientData,
        &queue,
    ) {
        return Err(condition("IOERR", 17));
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let current = state.transient.queues.get(&queue).cloned();
    if let Some((_, existing_value)) = current
        .as_ref()
        .and_then(|queue| queue.records.iter().find(|(key, _)| key == effect_key))
    {
        if existing_value != &value {
            return Err(HostProblem::IdempotencyConflict);
        }
        return service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    if state
        .transient
        .queues
        .values()
        .map(|queue| queue.records.len())
        .sum::<usize>()
        >= service.limits.max_queue_records
        || state
            .transient
            .bytes
            .checked_add(value.len())
            .is_none_or(|bytes| bytes > service.limits.max_queue_bytes)
        || value.len() > definition.max_bytes
        || current.as_ref().is_some_and(|queue| {
            queue.records.len() >= definition.max_records
                || queue
                    .records
                    .iter()
                    .map(|(_, record)| record.len())
                    .sum::<usize>()
                    .checked_add(value.len())
                    .is_none_or(|bytes| bytes > definition.max_bytes)
        })
    {
        return Err(condition("NOSPACE", 18));
    }
    let mut next = current.unwrap_or(TransientQueue {
        records: Vec::new(),
        version: 0,
    });
    let expected = (next.version != 0).then_some(next.version);
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    next.records.push((effect_key.into(), value.clone()));
    persist_queue(service.store.as_ref(), queue.clone(), &next, expected)
        .map_err(mutation_problem)?;
    state.transient.queues.insert(queue, next);
    state.transient.bytes += value.len();
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

fn delete(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.keys().any(|name| {
        !matches!(
            name.as_str(),
            "QUEUE" | "TDQUEUE" | "SYSID" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
        )
    }) {
        return Err(HostProblem::Malformed);
    }
    let queue = queue_name(request)?;
    validate_local_system(run, request)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TD.{queue}"),
        AccessIntent::Update,
    )?;
    let mut state = service.lock()?;
    let definition = state.transient.definition(&queue, service.limits)?;
    validate_delete_definition(&definition)?;
    let current =
        state
            .transient
            .queues
            .get(&queue)
            .cloned()
            .ok_or_else(|| HostProblem::Condition {
                name: "QIDERR".into(),
                response: 44,
                response2: 0,
            })?;
    service
        .store
        .delete_provider_state("cics-tdq", &queue, current.version)
        .map_err(store_error)
        .map_err(mutation_problem)?;
    let released = current
        .records
        .iter()
        .try_fold(0usize, |total, (_, record)| total.checked_add(record.len()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    state.transient.queues.remove(&queue);
    state.transient.bytes = state
        .transient
        .bytes
        .checked_sub(released)
        .ok_or(HostProblem::InfrastructureFailure)?;
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
