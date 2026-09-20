use super::super::{
    CicsService, Run, TransientQueue, argument_bytes, argument_text, encode_transient,
    mutation_problem, store_error,
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
        CicsOperation::DeleteTransientData => delete(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
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
