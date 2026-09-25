use super::{
    CounterKind, MAX_CAS_RETRIES, authorize,
    define::number,
    get::{comparison, suppressed},
    invreq, replay_identity, response, selector, state, unavailable,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let (selector_name, kind) = match request.operation {
        CicsOperation::UpdateCounter => ("COUNTER", CounterKind::Fullword),
        CicsOperation::UpdateDCounter => ("DCOUNTER", CounterKind::Doubleword),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !request.arguments.contains_key(selector_name)
        || !request.arguments.contains_key("VALUE")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "COUNTER"
                    | "DCOUNTER"
                    | "POOL"
                    | "VALUE"
                    | "COMPAREMIN"
                    | "COMPAREMAX"
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
    let value = number(request, "VALUE", kind, 406)?.ok_or(HostProblem::Malformed)?;
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
        if (value < record.minimum && !(record.maximum == u64::MAX && value == 0))
            || u128::from(value) > u128::from(record.maximum) + 1
        {
            return Err(invreq(406));
        }
        if !comparison(record.current_value(), compare_min, compare_max) {
            return Err(suppressed(103));
        }
        let mut next = current.clone();
        let next_record = next
            .records
            .get_mut(key.as_str())
            .ok_or(HostProblem::InfrastructureFailure)?;
        next_record.set_current(value);
        next.record_replay(effect_key.clone(), replay.clone(), service.limits)?;
        if state::persist(service, &current, &mut next)? {
            return response(service, run, request, &replay.reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}
