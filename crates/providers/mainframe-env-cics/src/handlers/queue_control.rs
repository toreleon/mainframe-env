use super::super::{
    CicsService, Run, TransientQueue, argument_bytes, argument_text, decimal_payload,
    decode_transient, encode_transient, mutation_problem, store_error,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::ProviderStateRecord;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::WriteTransientData => write(service, run, request),
        CicsOperation::ReadTransientData => read(service, run, request),
        CicsOperation::DeleteTransientData => delete(service, run, request),
        CicsOperation::DeleteTemporaryStorage => delete_temporary(service, run, request),
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
                "QUEUE" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "INTO" | "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "INTO.MAXLENGTH" | "LENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
        || !request.arguments.contains_key("INTO")
        || !request.arguments.contains_key("INTO.MAXLENGTH")
    {
        return Err(HostProblem::Malformed);
    }
    let queue = queue_name(request)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TD.{queue}"),
        AccessIntent::Read,
    )?;
    let into_maximum = signed_decimal(request, "INTO.MAXLENGTH")?
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
    let current = state
        .transient
        .get(&queue)
        .cloned()
        .ok_or_else(|| HostProblem::Condition {
            name: "QIDERR".into(),
            response: 44,
            response2: 0,
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
    let maximum = requested
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(into_maximum)
        .min(into_maximum);
    let truncated = maximum < original.len();
    let mut value = original.clone();
    value.truncate(maximum);
    let mut next = current;
    next.records.remove(0);
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cics-tdq".into(),
                key: queue.clone(),
                version: next.version,
                payload: encode_transient(&next)?,
            },
            Some(next.version - 1),
        )
        .map_err(store_error)
        .map_err(mutation_problem)?;
    state.transient_bytes = state
        .transient_bytes
        .checked_sub(original.len())
        .ok_or(HostProblem::InfrastructureFailure)?;
    state.transient.insert(queue, next);
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        if truncated { "LENGERR" } else { "NORMAL" },
        if truncated { 22 } else { 0 },
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
    Ok(response)
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

fn validate_temporary_system(run: &Run, request: &CicsRequest) -> Result<(), HostProblem> {
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
            response2: 0,
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
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let current = state.transient.get(&queue).cloned();
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
        .values()
        .map(|queue| queue.records.len())
        .sum::<usize>()
        >= service.limits.max_queue_records
        || state
            .transient_bytes
            .checked_add(value.len())
            .is_none_or(|bytes| bytes > service.limits.max_queue_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
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
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cics-tdq".into(),
                key: queue.clone(),
                version: next.version,
                payload: encode_transient(&next)?,
            },
            expected,
        )
        .map_err(store_error)
        .map_err(mutation_problem)?;
    state.transient.insert(queue, next);
    state.transient_bytes += value.len();
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
            "QUEUE" | "TDQUEUE" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
        )
    }) {
        return Err(HostProblem::Malformed);
    }
    let queue = queue_name(request)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TD.{queue}"),
        AccessIntent::Update,
    )?;
    let mut state = service.lock()?;
    let current = state
        .transient
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
    state.transient.remove(&queue);
    state.transient_bytes = state
        .transient_bytes
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

fn delete_temporary(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "QNAME" | "QUEUE" | "SYSID" => false,
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    let queue = temporary_queue_name(request)?;
    validate_temporary_system(run, request)?;
    service.authorize(
        run,
        "QUEUE",
        &format!("CICS.TS.{queue}"),
        AccessIntent::Update,
    )?;
    let current = service
        .store
        .get_provider_state("cics-tsq", &queue)
        .map_err(store_error)?
        .ok_or_else(|| HostProblem::Condition {
            name: "QIDERR".into(),
            response: 44,
            response2: 0,
        })?;
    decode_transient(&current.payload, current.version, service.limits)?;
    service
        .store
        .delete_provider_state("cics-tsq", &queue, current.version)
        .map_err(store_error)
        .map_err(mutation_problem)?;
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
