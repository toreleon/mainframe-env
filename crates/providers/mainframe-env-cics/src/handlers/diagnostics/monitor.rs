use super::{response, state};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{
    AccessIntent, CicsRequest, CicsResponse, ClockRequest, HostProblem, HostRequest, HostResult,
    canonical_request_digest,
};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate(request)?;
    let point = decimal(request, "POINT")?;
    if !(1..=255).contains(&point) {
        return Err(condition(1));
    }
    let entry = match request.arguments.get("ENTRYNAME") {
        Some(value) if value.bytes().len() == 8 => String::from_utf8(value.bytes().to_vec())
            .map_err(|_| HostProblem::Malformed)?
            .trim_end()
            .to_ascii_uppercase(),
        Some(_) => return Err(HostProblem::Malformed),
        None => "USER".into(),
    };
    let definition_key = format!("{entry}:{point:03}");
    service.authorize(
        run,
        "CICSDIAG",
        &format!("CICS.DIAG.MONITOR.{entry}.{point:03}"),
        AccessIntent::Update,
    )?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let _guard = service.lock()?;
    let current = state::load(service)?;
    if let Some(reply) = current.replay(mutation.idempotency_key.as_str(), digest)? {
        return monitor_response(service, run, request, reply);
    }
    let definition = current
        .monitor_definitions
        .get(&definition_key)
        .ok_or_else(|| condition(2))?;
    let mut next = current.clone();
    let mut reply = state::DiagnosticReply::normal();
    let slot_key = |slot: u16| format!("{entry}:{slot:03}");
    match definition.action {
        state::CicsMonitorAction::AddCounter { slot }
        | state::CicsMonitorAction::SubtractCounter { slot }
        | state::CicsMonitorAction::OrCounter { slot } => {
            let raw = data_word(request, "DATA1", 3, 5)?;
            if request.arguments.contains_key("DATA2") {
                data_word(request, "DATA2", 4, 6)?;
                return Err(condition(4));
            }
            let key = slot_key(slot);
            let prior = *next.monitor_counters.get(&key).unwrap_or(&0);
            let value = match definition.action {
                state::CicsMonitorAction::AddCounter { .. } => prior.checked_add(i64::from(raw)),
                state::CicsMonitorAction::SubtractCounter { .. } => {
                    prior.checked_sub(i64::from(raw))
                }
                state::CicsMonitorAction::OrCounter { .. } => {
                    Some(prior | i64::from(u32::from_be_bytes(raw.to_be_bytes())))
                }
                _ => unreachable!(),
            }
            .ok_or(HostProblem::ResourceExhausted)?;
            next.monitor_counters.insert(key, value);
        }
        state::CicsMonitorAction::StartClock { slot }
        | state::CicsMonitorAction::StopClock { slot } => {
            if request.arguments.contains_key("DATA1") || request.arguments.contains_key("DATA2") {
                return Err(condition(if request.arguments.contains_key("DATA1") {
                    3
                } else {
                    4
                }));
            }
            let now = match service.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
                HostResult::Clock(timestamp) => {
                    super::super::time::monitor_milliseconds(&timestamp)?
                }
                _ => return Err(HostProblem::ProviderFailure),
            };
            let key = slot_key(slot);
            if matches!(
                definition.action,
                state::CicsMonitorAction::StartClock { .. }
            ) {
                next.monitor_clocks.insert(key, now);
            } else {
                let started = next
                    .monitor_clocks
                    .remove(&key)
                    .ok_or_else(|| condition(3))?;
                let elapsed = now
                    .checked_sub(started)
                    .ok_or(HostProblem::ProviderFailure)?;
                next.monitor_counters.insert(
                    key,
                    i64::try_from(elapsed).map_err(|_| HostProblem::ResourceExhausted)?,
                );
            }
        }
        state::CicsMonitorAction::Move {
            offset,
            maximum_length,
        } => {
            data_word(request, "DATA1", 3, 5)?;
            let bytes = request
                .arguments
                .get("DATA1.BYTES")
                .ok_or_else(|| condition(3))?
                .bytes();
            let length = if request.arguments.contains_key("DATA2") {
                let length = data_word(request, "DATA2", 4, 6)?;
                if length < 0 {
                    return Err(condition(4));
                }
                if length == 0 {
                    usize::from(maximum_length)
                } else {
                    usize::try_from(length).map_err(|_| condition(4))?
                }
            } else {
                reply.condition = "INVREQ".into();
                reply.response = 16;
                reply.response2 = 6;
                usize::from(maximum_length)
            };
            if length > usize::from(maximum_length) || length > bytes.len() {
                return Err(condition(4));
            }
            let text = next.monitor_text.entry(entry.clone()).or_default();
            let end = usize::from(offset)
                .checked_add(length)
                .ok_or(HostProblem::ResourceExhausted)?;
            if end > 8192 {
                return Err(condition(4));
            }
            text.resize(text.len().max(end), 0);
            text[usize::from(offset)..end].copy_from_slice(&bytes[..length]);
        }
    }
    next.record_replay(
        mutation.idempotency_key.as_str(),
        digest,
        reply.clone(),
        service.limits,
    )?;
    state::persist(service, current.version, &mut next)?;
    monitor_response(service, run, request, reply)
}

fn monitor_response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    reply: state::DiagnosticReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.response != 0 {
        return super::super::condition(
            service,
            run,
            &request.condition_policy,
            HostProblem::Condition {
                name: reply.condition,
                response: reply.response,
                response2: reply.response2,
            },
        );
    }
    response(service, run, reply)
}

fn validate(request: &CicsRequest) -> Result<(), HostProblem> {
    if request
        .arguments
        .iter()
        .any(|(name, value)| match name.as_str() {
            "POINT" => value.schema() != "mainframe-env.cics.decimal@1",
            "DATA1" | "DATA2" => {
                value.schema() != "mainframe-env.cics.storage-value@1" || value.bytes().len() != 4
            }
            "DATA1.BYTES" => {
                value.schema() != "mainframe-env.cics.monitor-data@1" || value.bytes().len() > 8192
            }
            "ENTRYNAME" => {
                !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ) || value.bytes().len() != 8
            }
            "OPTION.NOHANDLE" => {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            }
            "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
            _ => true,
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)
}

fn data_word(
    request: &CicsRequest,
    name: &str,
    invalid: i32,
    missing: i32,
) -> Result<i32, HostProblem> {
    let value = request
        .arguments
        .get(name)
        .ok_or_else(|| condition(missing))?;
    let bytes: [u8; 4] = value.bytes().try_into().map_err(|_| condition(invalid))?;
    Ok(i32::from_be_bytes(bytes))
}

fn condition(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}
