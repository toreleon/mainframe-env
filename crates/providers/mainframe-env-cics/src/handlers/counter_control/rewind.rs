use super::{
    CounterKind, MAX_CAS_RETRIES, authorize, define::number, get::suppressed, invreq,
    replay_identity, response, selector, state, unavailable,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let (selector_name, kind) = match request.operation {
        CicsOperation::RewindCounter => ("COUNTER", CounterKind::Fullword),
        CicsOperation::RewindDCounter => ("DCOUNTER", CounterKind::Doubleword),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !request.arguments.contains_key(selector_name)
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "COUNTER"
                    | "DCOUNTER"
                    | "POOL"
                    | "INCREMENT"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOSUSPEND"
                    | "OPTION.NOHANDLE"
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
    let increment = number(request, "INCREMENT", kind, 406)?.unwrap_or(0);
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
        if !record.at_limit
            && u128::from(record.current) + u128::from(increment) < u128::from(record.maximum) + 1
        {
            return Err(suppressed(102));
        }
        let mut next = current.clone();
        let next_record = next
            .records
            .get_mut(key.as_str())
            .ok_or(HostProblem::InfrastructureFailure)?;
        next_record.set_current(record.minimum);
        next.record_replay(effect_key.clone(), replay.clone(), service.limits)?;
        if state::persist(service, &current, &mut next)? {
            return response(service, run, request, &replay.reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}
