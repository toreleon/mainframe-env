use super::{
    CounterKind, CounterRecord, MAX_CAS_RETRIES, authorize, invreq, replay_identity, response,
    selector, state, unavailable,
};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{AccessIntent, CicsOperation, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let (pool, key) = selector(request)?;
    authorize(service, run, &key, AccessIntent::Update)?;
    let kind = if request.operation == CicsOperation::DefineCounter {
        CounterKind::Fullword
    } else {
        CounterKind::Doubleword
    };
    let minimum = number(request, "MINIMUM", kind, 407)?.unwrap_or(0);
    let mut maximum = number(request, "MAXIMUM", kind, 407)?.unwrap_or(match kind {
        CounterKind::Fullword => i32::MAX as u64,
        CounterKind::Doubleword => u64::MAX,
    });
    if minimum > maximum {
        return Err(invreq(407));
    }
    if kind == CounterKind::Doubleword {
        // The named-counter server reserves two sentinel values at the end
        // of the unsigned range. The pinned DEFINE topic enumerates these cases.
        if minimum == 0 && maximum >= u64::MAX - 1 {
            maximum = u64::MAX - 2;
        } else if minimum == 1 && maximum == u64::MAX {
            maximum = u64::MAX - 1;
        }
    }
    let value = number(request, "VALUE", kind, 406)?.unwrap_or(0);
    if value < minimum || u128::from(value) > u128::from(maximum) + 1 {
        return Err(invreq(406));
    }
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
        if current.records.contains_key(key.as_str()) {
            return Err(invreq(202));
        }
        if current.records.len() >= service.limits.max_queue_records {
            return Err(invreq(302));
        }
        let mut next = current.clone();
        next.records.insert(
            key.as_str().into(),
            CounterRecord {
                kind,
                current: value,
                at_limit: value == maximum.wrapping_add(1) && maximum != u64::MAX,
                minimum,
                maximum,
            },
        );
        next.record_replay(effect_key.clone(), replay.clone(), service.limits)?;
        if state::persist(service, &current, &mut next)? {
            return response(service, run, request, &replay.reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let selector = match request.operation {
        CicsOperation::DefineCounter => "COUNTER",
        CicsOperation::DefineDCounter => "DCOUNTER",
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !request.arguments.contains_key(selector)
        || request.arguments.contains_key("MINIMUM") && !request.arguments.contains_key("VALUE")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "COUNTER"
                    | "DCOUNTER"
                    | "POOL"
                    | "VALUE"
                    | "MINIMUM"
                    | "MAXIMUM"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOSUSPEND"
                    | "OPTION.NOHANDLE"
            ) || name
                == if selector == "COUNTER" {
                    "DCOUNTER"
                } else {
                    "COUNTER"
                }
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(super) fn number(
    request: &CicsRequest,
    name: &str,
    kind: CounterKind,
    response2: i32,
) -> Result<Option<u64>, HostProblem> {
    let Some(value) = request.arguments.get(name) else {
        return Ok(None);
    };
    let text = std::str::from_utf8(value.bytes()).map_err(|_| invreq(response2))?;
    let number = match kind {
        CounterKind::Fullword => text.parse::<i32>().ok().and_then(|number| {
            if number >= 0 {
                Some(number as u64)
            } else if name == "VALUE" && number == i32::MIN {
                Some(i32::MAX as u64 + 1)
            } else {
                None
            }
        }),
        CounterKind::Doubleword => text.parse::<u64>().ok(),
    }
    .ok_or_else(|| invreq(response2))?;
    Ok(Some(number))
}
