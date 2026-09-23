use super::{
    CounterKind, CounterReply, MAX_CAS_RETRIES, authorize, define::number, invreq, replay_identity,
    response, selector, state, unavailable,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let (selector_name, kind) = match request.operation {
        CicsOperation::GetCounter => ("COUNTER", CounterKind::Fullword),
        CicsOperation::GetDCounter => ("DCOUNTER", CounterKind::Doubleword),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !request.arguments.contains_key(selector_name)
        || !request.arguments.contains_key("VALUE")
        || !request.arguments["VALUE"].bytes().is_empty()
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "COUNTER"
                    | "DCOUNTER"
                    | "POOL"
                    | "VALUE"
                    | "INCREMENT"
                    | "COMPAREMIN"
                    | "COMPAREMAX"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOSUSPEND"
                    | "OPTION.NOHANDLE"
                    | "OPTION.REDUCE"
                    | "OPTION.WRAP"
            ) || name
                == if selector_name == "COUNTER" {
                    "DCOUNTER"
                } else {
                    "COUNTER"
                }
        })
    {
        return Err(HostProblem::Malformed);
    }
    let (pool, key) = selector(request)?;
    authorize(service, run, &key, AccessIntent::Update)?;
    let increment = number(request, "INCREMENT", kind, 406)?.unwrap_or(1);
    let compare_min = number(request, "COMPAREMIN", kind, 406)?;
    let compare_max = number(request, "COMPAREMAX", kind, 406)?;
    let (effect_key, replay) = replay_identity(run, request)?;
    for _ in 0..MAX_CAS_RETRIES {
        let current = state::load(service.store.as_ref(), service.limits)?;
        if let Some(saved) = current.replay(
            &effect_key,
            &replay.owner_execution,
            &replay.owner_run_unit,
            &replay.owner_principal,
            replay.request_digest,
        )? {
            return response(service, run, request, &saved);
        }
        if current.rebuilding.contains(&pool) {
            return unavailable(service, run, request);
        }
        let record = current
            .records
            .get(key.as_str())
            .ok_or_else(|| invreq(201))?;
        let range = u128::from(record.maximum) - u128::from(record.minimum) + 1;
        if u128::from(increment) > range {
            return Err(invreq(406));
        }
        let wrap = request.arguments.contains_key("OPTION.WRAP");
        let reduce = request.arguments.contains_key("OPTION.REDUCE");
        if record.at_limit && !wrap {
            return Err(suppressed(101));
        }
        let mut value = if record.at_limit {
            record.minimum
        } else {
            record.current
        };
        let mut remaining = u128::from(record.maximum) + 1 - u128::from(value);
        if u128::from(increment) > remaining {
            if reduce && remaining > 0 {
                // The remaining range is reserved and the next request sees at-limit.
            } else if wrap {
                value = record.minimum;
                remaining = range;
            } else {
                return Err(suppressed(101));
            }
        }
        if !comparison(value, compare_min, compare_max) {
            return Err(suppressed(103));
        }
        let mut next = current.clone();
        let next_record = next
            .records
            .get_mut(key.as_str())
            .ok_or(HostProblem::InfrastructureFailure)?;
        if u128::from(increment) >= remaining {
            next_record.reach_limit();
        } else {
            next_record.set_current(value + increment);
        }
        let mut reply = CounterReply::normal();
        reply.value = Some(value);
        if kind == CounterKind::Fullword && value > i32::MAX as u64 {
            reply.condition = "LENGERR".into();
            reply.response = 22;
            reply.response2 = if value <= u32::MAX as u64 {
                1
            } else if value < (1u64 << 33) {
                2
            } else {
                3
            };
        }
        let mut saved_replay = replay.clone();
        saved_replay.reply = reply.clone();
        next.record_replay(effect_key.clone(), saved_replay, service.limits)?;
        if state::persist(service, &current, &mut next)? {
            return response(service, run, request, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

pub(super) fn comparison(value: u64, minimum: Option<u64>, maximum: Option<u64>) -> bool {
    match (minimum, maximum) {
        (Some(minimum), Some(maximum)) if minimum > maximum => value >= minimum || value <= maximum,
        (Some(minimum), Some(maximum)) => value >= minimum && value <= maximum,
        (Some(minimum), None) => value >= minimum,
        (None, Some(maximum)) => value <= maximum,
        (None, None) => true,
    }
}

pub(super) fn suppressed(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "SUPPRESSED".into(),
        response: 72,
        response2,
    }
}
